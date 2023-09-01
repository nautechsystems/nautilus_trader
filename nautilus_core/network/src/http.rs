// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{collections::HashMap, sync::Arc};

use futures_util::{stream, StreamExt};
use hyper::{Body, Client, Method, Request, Response};
use hyper_tls::HttpsConnector;
use pyo3::{exceptions::PyException, prelude::*, types::PyBytes};

use crate::ratelimiter::{clock::MonotonicClock, quota::Quota, RateLimiter};

/// Provides a high-performance HttpClient for HTTP requests.
///
/// The client is backed by a hyper Client which keeps connections alive and
/// can be cloned cheaply. The client also has a list of header fields to
/// extract from the response.
///
/// The client returns an [HttpResponse]. The client filters only the key value
/// for the give `header_keys`.
#[derive(Clone)]
pub struct InnerHttpClient {
    client: Client<HttpsConnector<hyper::client::HttpConnector>>,
    header_keys: Vec<String>,
}

#[pyclass]
pub struct HttpClient {
    rate_limiter: Arc<RateLimiter<String, MonotonicClock>>,
    client: InnerHttpClient,
}

#[pyclass]
#[derive(Debug, Clone, Copy)]
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
            HttpMethod::GET => Method::GET,
            HttpMethod::POST => Method::POST,
            HttpMethod::PUT => Method::PUT,
            HttpMethod::DELETE => Method::DELETE,
            HttpMethod::PATCH => Method::PATCH,
        }
    }
}

/// HttpResponse contains relevant data from a HTTP request.
#[pyclass]
#[derive(Debug, Clone)]
pub struct HttpResponse {
    #[pyo3(get)]
    pub status: u16,
    #[pyo3(get)]
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

impl Default for InnerHttpClient {
    fn default() -> Self {
        let https = HttpsConnector::new();
        let client = Client::builder().build::<_, hyper::Body>(https);
        Self {
            client,
            header_keys: Default::default(),
        }
    }
}

#[pymethods]
impl HttpResponse {
    #[getter]
    fn get_body(&self, py: Python) -> PyResult<Py<PyBytes>> {
        Ok(PyBytes::new(py, &self.body).into())
    }
}

#[pymethods]
impl HttpClient {
    /// Create a new HttpClient
    ///
    /// * `header_keys` - key value pairs for the given `header_keys` are retained from the responses.
    /// * `keyed_quota` - list of string quota pairs that gives quota for specific key values
    /// * `default_quota` - the default rate limiting quota for any request.
    ///   Default quota is optional and no quota is passthrough.
    #[new]
    #[pyo3(signature = (header_keys = Vec::new(), keyed_quotas = Vec::new(), default_quota = None))]
    pub fn py_new(
        header_keys: Vec<String>,
        keyed_quotas: Vec<(String, Quota)>,
        default_quota: Option<Quota>,
    ) -> Self {
        let https = HttpsConnector::new();
        let client = Client::builder().build::<_, hyper::Body>(https);
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

    /// Send an HTTP request
    ///
    /// * `method` - the HTTP method to call
    /// * `url` - the request is sent to this url
    /// * `keys` - the keys used for rate limiting the request
    /// * `headers` - the header key value pairs in the request
    /// * `body` - the bytes sent in the body of request
    pub fn request<'py>(
        &self,
        method: HttpMethod,
        url: String,
        keys: Vec<String>,
        headers: HashMap<String, String>,
        body: Option<&'py PyBytes>,
        py: Python<'py>,
    ) -> PyResult<&'py PyAny> {
        let body_vec = body.map(|py_bytes| py_bytes.as_bytes().to_vec());
        let client = self.client.clone();
        let rate_limiter = self.rate_limiter.clone();
        let method = method.into();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            // check keys for rate limiting quota
            let tasks = keys.iter().map(|key| rate_limiter.until_key_ready(key));
            stream::iter(tasks)
                .for_each(|key| async move {
                    key.await;
                })
                .await;
            match client.send_request(method, url, headers, body_vec).await {
                Ok(res) => Ok(res),
                Err(e) => Err(PyErr::new::<PyException, _>(format!(
                    "Error handling repsonse: {e}"
                ))),
            }
        })
    }
}

impl InnerHttpClient {
    pub async fn send_request(
        &self,
        method: Method,
        url: String,
        headers: HashMap<String, String>,
        body: Option<Vec<u8>>,
    ) -> Result<HttpResponse, Box<dyn std::error::Error + Send + Sync>> {
        let mut req_builder = Request::builder().method(method).uri(url);

        for (header_name, header_value) in &headers {
            req_builder = req_builder.header(header_name, header_value);
        }

        let req = if let Some(body) = body {
            req_builder.body(Body::from(body))?
        } else {
            req_builder.body(Body::empty())?
        };

        let res = self.client.request(req).await?;
        self.to_response(res).await
    }

    pub async fn to_response(
        &self,
        res: Response<Body>,
    ) -> Result<HttpResponse, Box<dyn std::error::Error + Send + Sync>> {
        let headers: HashMap<String, String> = self
            .header_keys
            .iter()
            .filter_map(|key| res.headers().get(key).map(|val| (key, val)))
            .filter_map(|(key, val)| val.to_str().map(|v| (key, v)).ok())
            .map(|(k, v)| (k.clone(), v.to_owned()))
            .collect();
        let status = res.status().as_u16();
        let bytes = hyper::body::to_bytes(res.into_body()).await?;

        Ok(HttpResponse {
            status,
            headers,
            body: bytes.to_vec(),
        })
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::{
        convert::Infallible,
        net::{SocketAddr, TcpListener},
    };

    use hyper::{
        service::{make_service_fn, service_fn},
        Body, Method, Request, Response, Server, StatusCode,
    };
    use tokio::sync::oneshot;

    use super::*;

    async fn handle(req: Request<Body>) -> Result<Response<Body>, Infallible> {
        match (req.method(), req.uri().path()) {
            (&Method::GET, "/get") => {
                let response = Response::new(Body::from("hello-world!"));
                Ok(response)
            }
            (&Method::POST, "/post") => {
                let response = Response::builder()
                    .status(StatusCode::OK)
                    .body(Body::empty())
                    .unwrap();
                Ok(response)
            }
            (&Method::PATCH, "/patch") => {
                let response = Response::builder()
                    .status(StatusCode::OK)
                    .body(Body::empty())
                    .unwrap();
                Ok(response)
            }
            (&Method::DELETE, "/delete") => {
                let response = Response::builder()
                    .status(StatusCode::OK)
                    .body(Body::empty())
                    .unwrap();
                Ok(response)
            }
            _ => {
                let response = Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Body::empty())
                    .unwrap();
                Ok(response)
            }
        }
    }

    fn get_unique_port() -> u16 {
        // Create a temporary TcpListener to get an available port
        let listener =
            TcpListener::bind("127.0.0.1:0").expect("Failed to bind temporary TcpListener");
        let port = listener.local_addr().unwrap().port();

        // Close the listener to free up the port
        drop(listener);

        port
    }

    fn start_test_server() -> (SocketAddr, oneshot::Sender<()>) {
        let addr: SocketAddr = ([127, 0, 0, 1], get_unique_port()).into();
        let make_svc = make_service_fn(|_conn| async { Ok::<_, Infallible>(service_fn(handle)) });

        let (tx, rx) = oneshot::channel::<()>();

        let server = Server::bind(&addr).serve(make_svc);

        let graceful = server.with_graceful_shutdown(async {
            if let Err(e) = rx.await {
                eprintln!("shutdown signal error: {e}");
            }
        });

        tokio::spawn(async {
            if let Err(e) = graceful.await {
                eprintln!("server error: {e}");
            }
        });

        (addr, tx)
    }

    #[tokio::test]
    async fn test_get() {
        let (addr, _shutdown_tx) = start_test_server();
        let url = format!("http://{}:{}", addr.ip(), addr.port());

        let client = InnerHttpClient::default();
        let response = client
            .send_request(Method::GET, format!("{url}/get"), HashMap::new(), None)
            .await
            .unwrap();

        assert_eq!(response.status, StatusCode::OK);
        assert_eq!(String::from_utf8_lossy(&response.body), "hello-world!");
    }

    #[tokio::test]
    async fn test_post() {
        let (addr, _shutdown_tx) = start_test_server();
        let url = format!("http://{}:{}", addr.ip(), addr.port());

        let client = InnerHttpClient::default();
        let response = client
            .send_request(Method::POST, format!("{url}/post"), HashMap::new(), None)
            .await
            .unwrap();

        assert_eq!(response.status, StatusCode::OK);
    }

    #[tokio::test]
    async fn test_post_with_body() {
        let (addr, _shutdown_tx) = start_test_server();
        let url = format!("http://{}:{}", addr.ip(), addr.port());

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
                HashMap::new(),
                Some(body_bytes),
            )
            .await
            .unwrap();

        assert_eq!(response.status, StatusCode::OK);
    }

    #[tokio::test]
    async fn test_patch() {
        let (addr, _shutdown_tx) = start_test_server();
        let url = format!("http://{}:{}", addr.ip(), addr.port());

        let client = InnerHttpClient::default();
        let response = client
            .send_request(Method::PATCH, format!("{url}/patch"), HashMap::new(), None)
            .await
            .unwrap();

        assert_eq!(response.status, StatusCode::OK);
    }

    #[tokio::test]
    async fn test_delete() {
        let (addr, _shutdown_tx) = start_test_server();
        let url = format!("http://{}:{}", addr.ip(), addr.port());

        let client = InnerHttpClient::default();
        let response = client
            .send_request(
                Method::DELETE,
                format!("{url}/delete"),
                HashMap::new(),
                None,
            )
            .await
            .unwrap();

        assert_eq!(response.status, StatusCode::OK);
    }
}

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

use std::{collections::HashMap, str::FromStr};

use hyper::{Body, Client, Method, Request, Response};
use hyper_tls::HttpsConnector;
use pyo3::{exceptions::PyException, prelude::*, types::PyBytes};

/// Provides a high-performance HttpClient for HTTP requests.
///
/// The client is backed by a hyper Client which keeps connections alive and
/// can be cloned cheaply. The client also has a list of header fields to
/// extract from the response.
///
/// The client returns an [HttpResponse]. The client filters only the key value
/// for the give `header_keys`.
#[pyclass]
#[derive(Clone)]
pub struct HttpClient {
    client: Client<HttpsConnector<hyper::client::HttpConnector>>,
    header_keys: Vec<String>,
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

impl Default for HttpClient {
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
    #[new]
    #[pyo3(signature=(header_keys=[].to_vec()))]
    pub fn py_new(header_keys: Vec<String>) -> Self {
        let https = HttpsConnector::new();
        let client = Client::builder().build::<_, hyper::Body>(https);

        Self {
            client,
            header_keys,
        }
    }

    pub fn request<'py>(
        slf: PyRef<'_, Self>,
        method_str: String,
        url: String,
        headers: HashMap<String, String>,
        body: Option<&'py PyBytes>,
        py: Python<'py>,
    ) -> PyResult<&'py PyAny> {
        let method: Method = Method::from_str(&method_str.to_uppercase())
            .unwrap_or_else(|_| panic!("Invalid HTTP method {method_str}"));

        let body_vec = body.map(|py_bytes| py_bytes.as_bytes().to_vec());
        let client = slf.clone();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            match client.send_request(method, url, headers, body_vec).await {
                Ok(res) => Ok(res),
                Err(e) => Err(PyErr::new::<PyException, _>(format!(
                    "Error handling repsonse: {e}"
                ))),
            }
        })
    }

    pub fn get<'py>(
        slf: PyRef<'_, Self>,
        url: String,
        headers: HashMap<String, String>,
        body: Option<&'py PyBytes>,
        py: Python<'py>,
    ) -> PyResult<&'py PyAny> {
        let body_vec = body.map(|py_bytes| py_bytes.as_bytes().to_vec());
        let client = slf.clone();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            match client
                .send_request(Method::GET, url, headers, body_vec)
                .await
            {
                Ok(res) => Ok(res),
                Err(e) => Err(PyErr::new::<PyException, _>(format!(
                    "Error handling repsonse: {e}"
                ))),
            }
        })
    }

    pub fn post<'py>(
        slf: PyRef<'_, Self>,
        url: String,
        headers: HashMap<String, String>,
        body: Option<&'py PyBytes>,
        py: Python<'py>,
    ) -> PyResult<&'py PyAny> {
        let body_vec = body.map(|py_bytes| py_bytes.as_bytes().to_vec());
        let client = slf.clone();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            match client
                .send_request(Method::POST, url, headers, body_vec)
                .await
            {
                Ok(res) => Ok(res),
                Err(e) => Err(PyErr::new::<PyException, _>(format!(
                    "Error handling repsonse: {e}"
                ))),
            }
        })
    }

    pub fn patch<'py>(
        slf: PyRef<'_, Self>,
        url: String,
        headers: HashMap<String, String>,
        body: Option<&'py PyBytes>,
        py: Python<'py>,
    ) -> PyResult<&'py PyAny> {
        let body_vec = body.map(|py_bytes| py_bytes.as_bytes().to_vec());
        let client = slf.clone();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            match client
                .send_request(Method::PATCH, url, headers, body_vec)
                .await
            {
                Ok(res) => Ok(res),
                Err(e) => Err(PyErr::new::<PyException, _>(format!(
                    "Error handling repsonse: {e}"
                ))),
            }
        })
    }

    pub fn delete<'py>(
        slf: PyRef<'_, Self>,
        url: String,
        headers: HashMap<String, String>,
        body: Option<&'py PyBytes>,
        py: Python<'py>,
    ) -> PyResult<&'py PyAny> {
        let body_vec = body.map(|py_bytes| py_bytes.as_bytes().to_vec());
        let client = slf.clone();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            match client
                .send_request(Method::DELETE, url, headers, body_vec)
                .await
            {
                Ok(res) => Ok(res),
                Err(e) => Err(PyErr::new::<PyException, _>(format!(
                    "Error handling repsonse: {e}"
                ))),
            }
        })
    }
}

impl HttpClient {
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

        let client = HttpClient::default();
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

        let client = HttpClient::default();
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

        let client = HttpClient::default();

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

        let client = HttpClient::default();
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

        let client = HttpClient::default();
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

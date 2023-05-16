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
use pyo3::{
    exceptions::{PyException, PyTypeError},
    prelude::*,
    types::PyBytes,
    types::PyDict,
};

/// HttpClient makes HTTP requests to exchanges.
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
    #[must_use]
    pub fn new(header_keys: Vec<String>) -> Self {
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
        body: Option<&'py PyDict>,
        py: Python<'py>,
    ) -> PyResult<&'py PyAny> {
        let method: Method =
            Method::from_str(&method_str.to_lowercase()).expect("Invalid HTTP method {method_str}");
        let body: Option<HashMap<String, serde_json::Value>> = py_dict_to_rust_map(body)?;
        let client = slf.clone();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            match client.send_request(method, url, headers, body).await {
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
        body: Option<&'py PyDict>,
        py: Python<'py>,
    ) -> PyResult<&'py PyAny> {
        let body: Option<HashMap<String, serde_json::Value>> = py_dict_to_rust_map(body)?;
        let client = slf.clone();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            match client.send_request(Method::GET, url, headers, body).await {
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
        body: Option<&'py PyDict>,
        py: Python<'py>,
    ) -> PyResult<&'py PyAny> {
        let body: Option<HashMap<String, serde_json::Value>> = py_dict_to_rust_map(body)?;
        let client = slf.clone();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            match client.send_request(Method::POST, url, headers, body).await {
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
        body: Option<&'py PyDict>,
        py: Python<'py>,
    ) -> PyResult<&'py PyAny> {
        let body: Option<HashMap<String, serde_json::Value>> = py_dict_to_rust_map(body)?;
        let client = slf.clone();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            match client.send_request(Method::PATCH, url, headers, body).await {
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
        body: Option<&'py PyDict>,
        py: Python<'py>,
    ) -> PyResult<&'py PyAny> {
        let body: Option<HashMap<String, serde_json::Value>> = py_dict_to_rust_map(body)?;
        let client = slf.clone();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            match client
                .send_request(Method::DELETE, url, headers, body)
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
        body: Option<HashMap<String, serde_json::Value>>,
    ) -> Result<HttpResponse, Box<dyn std::error::Error + Send + Sync>> {
        let mut req_builder = Request::builder().method(method).uri(url);

        for (header_name, header_value) in &headers {
            req_builder = req_builder.header(header_name, header_value);
        }

        let req = if let Some(body) = body {
            let body = serde_json::to_string(&body)?;
            req_builder
                .header("Content-Type", "application/json")
                .body(Body::from(body))?
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

fn py_dict_to_rust_map(
    py_dict: Option<&PyDict>,
) -> PyResult<Option<HashMap<String, serde_json::Value>>> {
    match py_dict {
        Some(dict) => {
            let mut rust_map = HashMap::new();
            for (key, value) in dict {
                let key_str: String = key.extract().map_err(|e| {
                    PyErr::new::<PyTypeError, _>(format!("Failed to extract `body` key: {}", e))
                })?;
                let value_str: String = value.extract().map_err(|e| {
                    PyErr::new::<PyTypeError, _>(format!("Failed to extract `body` value: {}", e))
                })?;
                rust_map.insert(key_str, serde_json::Value::String(value_str));
            }
            Ok(Some(rust_map))
        }
        None => Ok(None),
    }
}

// Uncomment to change for module name for reduced debug builds in testing
#[pymodule]
pub fn nautilus_network(_: Python<'_>, m: &PyModule) -> PyResult<()> {
    m.add_class::<HttpClient>()?;
    m.add_class::<HttpResponse>()?;
    Ok(())
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::*;
    use httptest::{matchers::*, responders::*, Expectation, Server};
    use hyper::StatusCode;
    use pyo3::types::IntoPyDict;
    use serde_json::Value;

    #[test]
    fn test_py_dict_to_rust_map() {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let py_dict = [("key1", "value1"), ("key2", "value2")].into_py_dict(py);
            let result = py_dict_to_rust_map(Some(py_dict)).unwrap();

            let mut expected = HashMap::new();
            expected.insert(String::from("key1"), Value::String(String::from("value1")));
            expected.insert(String::from("key2"), Value::String(String::from("value2")));

            assert_eq!(result, Some(expected));
        });
    }

    #[test]
    fn test_py_dict_to_rust_map_none() {
        let result = py_dict_to_rust_map(None::<&PyDict>).unwrap();
        let expected = None;

        assert_eq!(result, expected);
    }

    #[tokio::test]
    async fn test_get() {
        let server = Server::run();
        server.expect(
            Expectation::matching(request::method_path("GET", "/get"))
                .respond_with(status_code(StatusCode::OK.into()).body("hello-world!")),
        );
        let url = format!("http://{}", server.addr());

        let client = HttpClient::default();
        let response = client
            .send_request(Method::GET, format!("{}/get", url), HashMap::new(), None)
            .await
            .unwrap();

        assert_eq!(response.status, StatusCode::OK);
        assert_eq!(String::from_utf8_lossy(&response.body), "hello-world!");
    }

    #[tokio::test]
    async fn test_post() {
        let server = Server::run();
        server.expect(
            Expectation::matching(request::method_path("POST", "/post"))
                .respond_with(status_code(StatusCode::OK.into())),
        );
        let url = format!("http://{}", server.addr());

        let client = HttpClient::default();
        let response = client
            .send_request(Method::POST, format!("{}/post", url), HashMap::new(), None)
            .await
            .unwrap();

        assert_eq!(response.status, StatusCode::OK);
    }

    #[tokio::test]
    async fn test_post_with_body() {
        let server = Server::run();
        server.expect(
            Expectation::matching(request::method_path("POST", "/post"))
                .respond_with(status_code(StatusCode::OK.into())),
        );
        let url = format!("http://{}", server.addr());

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

        let response = client
            .send_request(
                Method::POST,
                format!("{}/post", url),
                HashMap::new(),
                Some(body),
            )
            .await
            .unwrap();

        assert_eq!(response.status, StatusCode::OK);
    }

    #[tokio::test]
    async fn test_patch() {
        let server = Server::run();
        server.expect(
            Expectation::matching(request::method_path("PATCH", "/patch"))
                .respond_with(status_code(StatusCode::OK.into())),
        );
        let url = format!("http://{}", server.addr());

        let client = HttpClient::default();
        let response = client
            .send_request(
                Method::PATCH,
                format!("{}/patch", url),
                HashMap::new(),
                None,
            )
            .await
            .unwrap();

        assert_eq!(response.status, StatusCode::OK);
    }

    #[tokio::test]
    async fn test_delete() {
        let server = Server::run();
        server.expect(
            Expectation::matching(request::method_path("DELETE", "/delete"))
                .respond_with(status_code(StatusCode::OK.into())),
        );
        let url = format!("http://{}", server.addr());

        let client = HttpClient::default();
        let response = client
            .send_request(
                Method::DELETE,
                format!("{}/delete", url),
                HashMap::new(),
                None,
            )
            .await
            .unwrap();

        assert_eq!(response.status, StatusCode::OK);
    }
}

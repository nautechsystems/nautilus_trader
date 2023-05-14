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
use pyo3::{prelude::*, types::PyBytes};

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
        py: Python<'py>,
    ) -> PyResult<&'py PyAny> {
        let method: Method =
            Method::from_str(&method_str.to_lowercase()).expect("Invalid HTTP method {method_str}");
        let client = slf.clone();

        pyo3_asyncio::tokio::future_into_py(py, async move {
            match client.send_request(method, url, headers).await {
                Ok(res) => Ok(res),
                Err(_) => {
                    // TODO: log error
                    panic!("Could not handle response");
                }
            }
        })
    }

    pub fn get<'py>(
        slf: PyRef<'_, Self>,
        url: String,
        headers: HashMap<String, String>,
        py: Python<'py>,
    ) -> PyResult<&'py PyAny> {
        let client = slf.clone();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            match client.send_request(Method::GET, url, headers).await {
                Ok(res) => Ok(res),
                Err(_) => {
                    // TODO: log error
                    panic!("Could not handle response");
                }
            }
        })
    }

    pub fn post<'py>(
        slf: PyRef<'_, Self>,
        url: String,
        headers: HashMap<String, String>,
        py: Python<'py>,
    ) -> PyResult<&'py PyAny> {
        let client = slf.clone();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            match client.send_request(Method::POST, url, headers).await {
                Ok(res) => Ok(res),
                Err(_) => {
                    // TODO: log error
                    panic!("Could not handle response");
                }
            }
        })
    }

    pub fn patch<'py>(
        slf: PyRef<'_, Self>,
        url: String,
        headers: HashMap<String, String>,
        py: Python<'py>,
    ) -> PyResult<&'py PyAny> {
        let client = slf.clone();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            match client.send_request(Method::PATCH, url, headers).await {
                Ok(res) => Ok(res),
                Err(_) => {
                    // TODO: log error
                    panic!("Could not handle response");
                }
            }
        })
    }

    pub fn delete<'py>(
        slf: PyRef<'_, Self>,
        url: String,
        headers: HashMap<String, String>,
        py: Python<'py>,
    ) -> PyResult<&'py PyAny> {
        let client = slf.clone();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            match client.send_request(Method::DELETE, url, headers).await {
                Ok(res) => Ok(res),
                Err(_) => {
                    // TODO: log error
                    panic!("Could not handle response");
                }
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
    ) -> Result<HttpResponse, Box<dyn std::error::Error + Send + Sync>> {
        let mut req_builder = Request::builder().method(method).uri(url);

        for (header_name, header_value) in &headers {
            req_builder = req_builder.header(header_name, header_value);
        }

        let req = req_builder.body(Body::empty())?;
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

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

use std::collections::HashMap;

use hyper::{Body, Client, Method, Request, Response};
use hyper_tls::HttpsConnector;
use pyo3::prelude::*;

#[pyclass]
#[derive(Clone)]
struct HttpClient {
    client: Client<HttpsConnector<hyper::client::HttpConnector>>,
}

#[pymethods]
impl HttpClient {
    #[new]
    fn new() -> Self {
        let https = HttpsConnector::new();
        let client = Client::builder().build::<_, hyper::Body>(https);

        Self { client }
    }

    pub fn request<'py>(
        slf: PyRef<'_, Self>,
        method_str: String,
        url: String,
        headers: HashMap<String, String>,
        py: Python<'py>,
    ) -> PyResult<&'py PyAny> {
        let method: Method = if method_str == "get" {
            Method::GET
        } else {
            Method::POST
        };
        let client = slf.clone();

        pyo3_asyncio::tokio::future_into_py(py, async move {
            match client.request_bytes(method, url, headers).await {
                Ok(bytes) => Ok(bytes),
                Err(_) => {
                    // TODO: log error
                    panic!("could not handle");
                }
            }
        })
    }
}

impl HttpClient {
    pub async fn request_bytes(
        &self,
        method: Method,
        url: String,
        headers: HashMap<String, String>,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        let res = self.send_request(method, url, headers).await?;
        let bytes = hyper::body::to_bytes(res.into_body()).await?;

        Ok(bytes.to_vec())
    }

    pub async fn send_request(
        &self,
        method: Method,
        url: String,
        headers: HashMap<String, String>,
    ) -> Result<Response<Body>, Box<dyn std::error::Error + Send + Sync>> {
        let mut req_builder = Request::builder().method(method).uri(url);

        for (header_name, header_value) in headers.iter() {
            req_builder = req_builder.header(header_name, header_value);
        }

        let req = req_builder.body(Body::empty())?;
        Ok(self.client.request(req).await?)
    }
}

/// Loaded as nautilus_pyo3.network
#[pymodule]
#[pyo3(name = "nautilus_network")]
pub fn persistence(_: Python<'_>, m: &PyModule) -> PyResult<()> {
    m.add_class::<HttpClient>()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, io, result};

    use hyper::{Client, Method, StatusCode};
    use pyo3::{prepare_freethreaded_python, Python};

    use crate::HttpClient;

    #[tokio::test]
    async fn rust_test() {
        let http_client = HttpClient::new();
        let response = http_client
            .send_request(Method::GET, "https://github.com".into(), HashMap::new())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}

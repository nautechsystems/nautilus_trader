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
    error::Error,
    fmt::Display,
    hash::{Hash, Hasher},
    sync::Arc,
};

use bytes::Bytes;
use futures_util::{stream, StreamExt};
use pyo3::{create_exception, exceptions::PyException, prelude::*, types::PyBytes};

use crate::{
    http::{HttpClient, HttpClientError, HttpMethod, HttpResponse, InnerHttpClient},
    ratelimiter::{quota::Quota, RateLimiter},
};

/// Python exception class for generic HTTP errors.
create_exception!(network, HttpError, PyException);

/// Python exception class for generic HTTP timeout errors.
create_exception!(network, HttpTimeoutError, PyException);

impl HttpClientError {
    pub fn into_py_err(self) -> PyErr {
        match self {
            HttpClientError::Error(e) => PyErr::new::<HttpError, _>(e),
            HttpClientError::TimeoutError(e) => PyErr::new::<HttpTimeoutError, _>(e.to_string()),
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

#[pymethods]
impl HttpResponse {
    #[new]
    pub fn py_new(status: u16, body: Vec<u8>) -> Self {
        Self {
            status,
            headers: HashMap::new(),
            body: Bytes::from(body),
        }
    }

    #[getter]
    #[pyo3(name = "status")]
    pub fn py_status(&self) -> u16 {
        self.status
    }

    #[getter]
    #[pyo3(name = "headers")]
    pub fn py_headers(&self) -> HashMap<String, String> {
        self.headers.clone()
    }

    #[getter]
    #[pyo3(name = "body")]
    pub fn py_body(&self) -> &[u8] {
        self.body.as_ref()
    }
}

#[pymethods]
impl HttpClient {
    /// Create a new HttpClient.
    ///
    /// `header_keys`: The key value pairs for the given `header_keys` are retained from the responses.
    /// `keyed_quota`: A list of string quota pairs that gives quota for specific key values.
    /// `default_quota`: The default rate limiting quota for any request.
    /// Default quota is optional and no quota is passthrough.
    ///
    /// Rate limiting can be configured on a per-endpoint basis by passing
    /// key-value pairs of endpoint URLs and their respective quotas.
    ///
    /// For /foo -> 10 reqs/sec configure limit with ("foo", Quota.rate_per_second(10))
    ///
    /// Hierarchical rate limiting can be achieved by configuring the quotas for
    /// each level.
    ///
    /// For /foo/bar -> 10 reqs/sec and /foo -> 20 reqs/sec configure limits for
    /// keys "foo/bar" and "foo" respectively.
    ///
    /// When a request is made the URL should be split into all the keys within it.
    ///
    /// For request /foo/bar, should pass keys ["foo/bar", "foo"] for rate limiting.
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
    /// `method`: The HTTP method to call.
    /// `url`: The request is sent to this url.
    /// `headers`: The header key value pairs in the request.
    /// `body`: The bytes sent in the body of request.
    /// `keys`: The keys used for rate limiting the request.
    ///
    /// # Example
    ///
    /// When a request is made the URL should be split into all relevant keys within it.
    ///
    /// For request /foo/bar, should pass keys ["foo/bar", "foo"] for rate limiting.
    #[pyo3(name = "request")]
    fn py_request<'py>(
        &self,
        method: HttpMethod,
        url: String,
        headers: Option<HashMap<String, String>>,
        body: Option<&'py PyBytes>,
        keys: Option<Vec<String>>,
        timeout_secs: Option<u64>,
        py: Python<'py>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let headers = headers.unwrap_or_default();
        let body_vec = body.map(|py_bytes| py_bytes.as_bytes().to_vec());
        let keys = keys.unwrap_or_default();
        let client = self.client.clone();
        let rate_limiter = self.rate_limiter.clone();
        let method = method.into();
        pyo3_asyncio_0_21::tokio::future_into_py(py, async move {
            // Check keys for rate limiting quota
            let tasks = keys.iter().map(|key| rate_limiter.until_key_ready(key));
            stream::iter(tasks)
                .for_each(|key| async move {
                    key.await;
                })
                .await;
            client
                .send_request(method, url, headers, body_vec, timeout_secs)
                .await
                .map_err(|e| e.into_py_err())
        })
    }
}

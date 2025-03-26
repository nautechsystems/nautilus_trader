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

use std::{
    collections::{HashMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
};

use bytes::Bytes;
use nautilus_core::python::to_pyvalue_err;
use pyo3::{create_exception, exceptions::PyException, prelude::*};

use crate::{
    http::{HttpClient, HttpClientError, HttpMethod, HttpResponse, HttpStatus},
    ratelimiter::quota::Quota,
};

// Python exception class for generic HTTP errors.
create_exception!(network, HttpError, PyException);

// Python exception class for generic HTTP timeout errors.
create_exception!(network, HttpTimeoutError, PyException);

impl HttpClientError {
    #[must_use]
    pub fn into_py_err(self) -> PyErr {
        match self {
            Self::Error(e) => PyErr::new::<HttpError, _>(e),
            Self::TimeoutError(e) => PyErr::new::<HttpTimeoutError, _>(e),
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
    /// Creates a new [`HttpResponse`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid `status` code.
    #[new]
    pub fn py_new(status: u16, body: Vec<u8>) -> PyResult<Self> {
        Ok(Self {
            status: HttpStatus::from(status).map_err(to_pyvalue_err)?,
            headers: HashMap::new(),
            body: Bytes::from(body),
        })
    }

    #[getter]
    #[pyo3(name = "status")]
    pub const fn py_status(&self) -> u16 {
        self.status.as_u16()
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
    /// Creates a new HttpClient.
    ///
    /// `default_headers`: The default headers to be used with every request.
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
    #[pyo3(signature = (default_headers=HashMap::new(), header_keys=Vec::new(), keyed_quotas=Vec::new(), default_quota=None, timeout_secs=None))]
    #[must_use]
    pub fn py_new(
        default_headers: HashMap<String, String>,
        header_keys: Vec<String>,
        keyed_quotas: Vec<(String, Quota)>,
        default_quota: Option<Quota>,
        timeout_secs: Option<u64>,
    ) -> Self {
        Self::new(
            default_headers,
            header_keys,
            keyed_quotas,
            default_quota,
            timeout_secs,
        )
    }

    /// Sends an HTTP request.
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
    #[allow(clippy::too_many_arguments)]
    #[pyo3(name = "request")]
    #[pyo3(signature = (method, url, headers=None, body=None, keys=None, timeout_secs=None))]
    fn py_request<'py>(
        &self,
        method: HttpMethod,
        url: String,
        headers: Option<HashMap<String, String>>,
        body: Option<Vec<u8>>,
        keys: Option<Vec<String>>,
        timeout_secs: Option<u64>,
        py: Python<'py>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        let rate_limiter = self.rate_limiter.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            rate_limiter.await_keys_ready(keys).await;
            client
                .send_request(method.into(), url, headers, body, timeout_secs)
                .await
                .map_err(HttpClientError::into_py_err)
        })
    }
}

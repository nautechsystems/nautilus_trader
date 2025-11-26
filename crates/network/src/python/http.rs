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
    fs::File,
    hash::{Hash, Hasher},
    io::copy,
    path::Path,
    time::Duration,
};

use bytes::Bytes;
use nautilus_core::{collections::into_ustr_vec, python::to_pyvalue_err};
use pyo3::{create_exception, exceptions::PyException, prelude::*, types::PyDict};
use reqwest::blocking::Client;

#[cfg(test)]
use crate::runtime::get_runtime;
use crate::{
    http::{HttpClient, HttpClientError, HttpMethod, HttpResponse, HttpStatus},
    ratelimiter::quota::Quota,
};

// Python exception class for generic HTTP errors.
create_exception!(network, HttpError, PyException);

// Python exception class for generic HTTP timeout errors.
create_exception!(network, HttpTimeoutError, PyException);

// Python exception class for invalid proxy configuration.
create_exception!(network, HttpInvalidProxyError, PyException);

// Python exception class for HTTP client build errors.
create_exception!(network, HttpClientBuildError, PyException);

impl HttpClientError {
    #[must_use]
    pub fn into_py_err(self) -> PyErr {
        match self {
            Self::Error(e) => PyErr::new::<HttpError, _>(e),
            Self::TimeoutError(e) => PyErr::new::<HttpTimeoutError, _>(e),
            Self::InvalidProxy(e) => PyErr::new::<HttpInvalidProxyError, _>(e),
            Self::ClientBuildError(e) => PyErr::new::<HttpClientBuildError, _>(e),
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
            status: HttpStatus::try_from(status).map_err(to_pyvalue_err)?,
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
    /// Creates a new `HttpClient`.
    ///
    /// Rate limiting can be configured on a per-endpoint basis by passing
    /// key-value pairs of endpoint URLs and their respective quotas.
    ///
    /// For /foo -> 10 reqs/sec configure limit with ("foo", `Quota.rate_per_second(10)`)
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
    ///
    /// # Errors
    ///
    /// - Returns `HttpInvalidProxyError` if the proxy URL is malformed.
    /// - Returns `HttpClientBuildError` if building the HTTP client fails.
    #[new]
    #[pyo3(signature = (default_headers=HashMap::new(), header_keys=Vec::new(), keyed_quotas=Vec::new(), default_quota=None, timeout_secs=None, proxy_url=None))]
    pub fn py_new(
        default_headers: HashMap<String, String>,
        header_keys: Vec<String>,
        keyed_quotas: Vec<(String, Quota)>,
        default_quota: Option<Quota>,
        timeout_secs: Option<u64>,
        proxy_url: Option<String>,
    ) -> PyResult<Self> {
        Self::new(
            default_headers,
            header_keys,
            keyed_quotas,
            default_quota,
            timeout_secs,
            proxy_url,
        )
        .map_err(HttpClientError::into_py_err)
    }

    #[allow(clippy::too_many_arguments)]
    #[pyo3(name = "request")]
    #[pyo3(signature = (method, url, params=None, headers=None, body=None, keys=None, timeout_secs=None))]
    fn py_request<'py>(
        &self,
        method: HttpMethod,
        url: String,
        params: Option<&Bound<'_, PyAny>>,
        headers: Option<HashMap<String, String>>,
        body: Option<Vec<u8>>,
        keys: Option<Vec<String>>,
        timeout_secs: Option<u64>,
        py: Python<'py>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        let rate_limiter = self.rate_limiter.clone();
        let params = params_to_hashmap(params)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let keys = keys.map(into_ustr_vec);
            rate_limiter.await_keys_ready(keys).await;
            client
                .send_request(
                    method.into(),
                    url,
                    params.as_ref(),
                    headers,
                    body,
                    timeout_secs,
                )
                .await
                .map_err(HttpClientError::into_py_err)
        })
    }

    #[pyo3(name = "get")]
    #[pyo3(signature = (url, params=None, headers=None, keys=None, timeout_secs=None))]
    fn py_get<'py>(
        &self,
        url: String,
        params: Option<&Bound<'_, PyAny>>,
        headers: Option<HashMap<String, String>>,
        keys: Option<Vec<String>>,
        timeout_secs: Option<u64>,
        py: Python<'py>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let params = params_to_hashmap(params)?;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .get(url, params.as_ref(), headers, timeout_secs, keys)
                .await
                .map_err(HttpClientError::into_py_err)
        })
    }

    #[allow(clippy::too_many_arguments)]
    #[pyo3(name = "post")]
    #[pyo3(signature = (url, params=None, headers=None, body=None, keys=None, timeout_secs=None))]
    fn py_post<'py>(
        &self,
        url: String,
        params: Option<&Bound<'_, PyAny>>,
        headers: Option<HashMap<String, String>>,
        body: Option<Vec<u8>>,
        keys: Option<Vec<String>>,
        timeout_secs: Option<u64>,
        py: Python<'py>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let params = params_to_hashmap(params)?;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .post(url, params.as_ref(), headers, body, timeout_secs, keys)
                .await
                .map_err(HttpClientError::into_py_err)
        })
    }

    #[allow(clippy::too_many_arguments)]
    #[pyo3(name = "patch")]
    #[pyo3(signature = (url, params=None, headers=None, body=None, keys=None, timeout_secs=None))]
    fn py_patch<'py>(
        &self,
        url: String,
        params: Option<&Bound<'_, PyAny>>,
        headers: Option<HashMap<String, String>>,
        body: Option<Vec<u8>>,
        keys: Option<Vec<String>>,
        timeout_secs: Option<u64>,
        py: Python<'py>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let params = params_to_hashmap(params)?;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .patch(url, params.as_ref(), headers, body, timeout_secs, keys)
                .await
                .map_err(HttpClientError::into_py_err)
        })
    }

    #[pyo3(name = "delete")]
    #[pyo3(signature = (url, params=None, headers=None, keys=None, timeout_secs=None))]
    fn py_delete<'py>(
        &self,
        url: String,
        params: Option<&Bound<'_, PyAny>>,
        headers: Option<HashMap<String, String>>,
        keys: Option<Vec<String>>,
        timeout_secs: Option<u64>,
        py: Python<'py>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let params = params_to_hashmap(params)?;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .delete(url, params.as_ref(), headers, timeout_secs, keys)
                .await
                .map_err(HttpClientError::into_py_err)
        })
    }
}

/// Converts Python dict params to HashMap<String, Vec<String>> for URL encoding.
///
/// Accepts a dict where values can be:
/// - Single values (str, int, float, bool) -> converted to single-item vec.
/// - Lists/tuples of values -> each item converted to string.
fn params_to_hashmap(
    params: Option<&Bound<'_, PyAny>>,
) -> PyResult<Option<HashMap<String, Vec<String>>>> {
    let Some(params) = params else {
        return Ok(None);
    };

    let Ok(dict) = params.cast::<PyDict>() else {
        return Err(PyErr::new::<pyo3::exceptions::PyTypeError, _>(
            "params must be a dict",
        ));
    };

    let mut result = HashMap::new();

    for (key, value) in dict {
        let key_str = key.str()?.to_str()?.to_string();

        if let Ok(seq) = value.cast::<pyo3::types::PySequence>() {
            // Exclude strings (which are technically sequences in Python)
            if !value.is_instance_of::<pyo3::types::PyString>() {
                let values: Vec<String> = (0..seq.len()?)
                    .map(|i| {
                        let item = seq.get_item(i)?;
                        Ok(item.str()?.to_str()?.to_string())
                    })
                    .collect::<PyResult<_>>()?;
                result.insert(key_str, values);
                continue;
            }
        }

        let value_str = value.str()?.to_str()?.to_string();
        result.insert(key_str, vec![value_str]);
    }

    Ok(Some(result))
}

/// Blocking HTTP GET request.
///
/// Creates an HttpClient internally and blocks on the async operation using a dedicated runtime.
///
/// # Errors
///
/// Returns an error if:
/// - The HTTP client fails to initialize.
/// - The HTTP request fails (e.g., network error, timeout, invalid URL).
/// - The server returns an error response.
/// - The params argument is not a dict.
///
/// # Panics
///
/// Panics if the spawned thread panics or runtime creation fails.
#[pyfunction]
#[pyo3(signature = (url, params=None, headers=None, timeout_secs=None))]
pub fn http_get(
    _py: Python<'_>,
    url: String,
    params: Option<&Bound<'_, PyAny>>,
    headers: Option<HashMap<String, String>>,
    timeout_secs: Option<u64>,
) -> PyResult<HttpResponse> {
    let params_map = params_to_hashmap(params)?;

    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create runtime");

        runtime.block_on(async {
            let client = HttpClient::new(HashMap::new(), vec![], vec![], None, timeout_secs, None)
                .map_err(HttpClientError::into_py_err)?;

            client
                .get(url, params_map.as_ref(), headers, timeout_secs, None)
                .await
                .map_err(HttpClientError::into_py_err)
        })
    })
    .join()
    .expect("Thread panicked")
}

/// Blocking HTTP POST request.
///
/// Creates an HttpClient internally and blocks on the async operation using a dedicated runtime.
///
/// # Errors
///
/// Returns an error if:
/// - The HTTP client fails to initialize.
/// - The HTTP request fails (e.g., network error, timeout, invalid URL).
/// - The server returns an error response.
/// - The params argument is not a dict.
///
/// # Panics
///
/// Panics if the spawned thread panics or runtime creation fails.
#[pyfunction]
#[pyo3(signature = (url, params=None, headers=None, body=None, timeout_secs=None))]
pub fn http_post(
    _py: Python<'_>,
    url: String,
    params: Option<&Bound<'_, PyAny>>,
    headers: Option<HashMap<String, String>>,
    body: Option<Vec<u8>>,
    timeout_secs: Option<u64>,
) -> PyResult<HttpResponse> {
    let params_map = params_to_hashmap(params)?;

    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create runtime");

        runtime.block_on(async {
            let client = HttpClient::new(HashMap::new(), vec![], vec![], None, timeout_secs, None)
                .map_err(HttpClientError::into_py_err)?;

            client
                .post(url, params_map.as_ref(), headers, body, timeout_secs, None)
                .await
                .map_err(HttpClientError::into_py_err)
        })
    })
    .join()
    .expect("Thread panicked")
}

/// Blocking HTTP PATCH request.
///
/// Creates an HttpClient internally and blocks on the async operation using a dedicated runtime.
///
/// # Errors
///
/// Returns an error if:
/// - The HTTP client fails to initialize.
/// - The HTTP request fails (e.g., network error, timeout, invalid URL).
/// - The server returns an error response.
/// - The params argument is not a dict.
///
/// # Panics
///
/// Panics if the spawned thread panics or runtime creation fails.
#[pyfunction]
#[pyo3(signature = (url, params=None, headers=None, body=None, timeout_secs=None))]
pub fn http_patch(
    _py: Python<'_>,
    url: String,
    params: Option<&Bound<'_, PyAny>>,
    headers: Option<HashMap<String, String>>,
    body: Option<Vec<u8>>,
    timeout_secs: Option<u64>,
) -> PyResult<HttpResponse> {
    let params_map = params_to_hashmap(params)?;

    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create runtime");

        runtime.block_on(async {
            let client = HttpClient::new(HashMap::new(), vec![], vec![], None, timeout_secs, None)
                .map_err(HttpClientError::into_py_err)?;

            client
                .patch(url, params_map.as_ref(), headers, body, timeout_secs, None)
                .await
                .map_err(HttpClientError::into_py_err)
        })
    })
    .join()
    .expect("Thread panicked")
}

/// Blocking HTTP DELETE request.
///
/// Creates an HttpClient internally and blocks on the async operation using a dedicated runtime.
///
/// # Errors
///
/// Returns an error if:
/// - The HTTP client fails to initialize.
/// - The HTTP request fails (e.g., network error, timeout, invalid URL).
/// - The server returns an error response.
/// - The params argument is not a dict.
///
/// # Panics
///
/// Panics if the spawned thread panics or runtime creation fails.
#[pyfunction]
#[pyo3(signature = (url, params=None, headers=None, timeout_secs=None))]
pub fn http_delete(
    _py: Python<'_>,
    url: String,
    params: Option<&Bound<'_, PyAny>>,
    headers: Option<HashMap<String, String>>,
    timeout_secs: Option<u64>,
) -> PyResult<HttpResponse> {
    let params_map = params_to_hashmap(params)?;

    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create runtime");

        runtime.block_on(async {
            let client = HttpClient::new(HashMap::new(), vec![], vec![], None, timeout_secs, None)
                .map_err(HttpClientError::into_py_err)?;

            client
                .delete(url, params_map.as_ref(), headers, timeout_secs, None)
                .await
                .map_err(HttpClientError::into_py_err)
        })
    })
    .join()
    .expect("Thread panicked")
}

/// Downloads a file from URL to filepath using streaming.
///
/// Uses `reqwest::blocking::Client` to stream the response directly to disk,
/// avoiding loading large files into memory.
///
/// # Errors
///
/// Returns an error if:
/// - Parent directories cannot be created.
/// - The HTTP client fails to build.
/// - The HTTP request fails (e.g., network error, timeout, invalid URL).
/// - The server returns a non-success status code.
/// - The file cannot be created or written to.
/// - The params argument is not a dict.
#[pyfunction]
#[pyo3(signature = (url, filepath, params=None, headers=None, timeout_secs=None))]
pub fn http_download(
    _py: Python<'_>,
    url: String,
    filepath: String,
    params: Option<&Bound<'_, PyAny>>,
    headers: Option<HashMap<String, String>>,
    timeout_secs: Option<u64>,
) -> PyResult<()> {
    let params_map = params_to_hashmap(params)?;

    // Encode params into URL manually for blocking client
    let full_url = if let Some(ref params) = params_map {
        // Flatten HashMap<String, Vec<String>> into Vec<(String, String)>
        let pairs: Vec<(String, String)> = params
            .iter()
            .flat_map(|(key, values)| values.iter().map(move |value| (key.clone(), value.clone())))
            .collect();

        if pairs.is_empty() {
            url
        } else {
            let query_string = serde_urlencoded::to_string(pairs).map_err(to_pyvalue_err)?;
            // Check if URL already has a query string
            let separator = if url.contains('?') { '&' } else { '?' };
            format!("{}{}{}", url, separator, query_string)
        }
    } else {
        url
    };

    let filepath = Path::new(&filepath);

    if let Some(parent) = filepath.parent() {
        std::fs::create_dir_all(parent).map_err(to_pyvalue_err)?;
    }

    let mut client_builder = Client::builder();
    if let Some(timeout) = timeout_secs {
        client_builder = client_builder.timeout(Duration::from_secs(timeout));
    }
    let client = client_builder.build().map_err(to_pyvalue_err)?;

    let mut request_builder = client.get(&full_url);
    if let Some(headers_map) = headers {
        for (key, value) in headers_map {
            request_builder = request_builder.header(key, value);
        }
    }

    let mut response = request_builder.send().map_err(to_pyvalue_err)?;

    if !response.status().is_success() {
        return Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!(
            "HTTP error: {}",
            response.status()
        )));
    }

    let mut file = File::create(filepath).map_err(to_pyvalue_err)?;
    copy(&mut response, &mut file).map_err(to_pyvalue_err)?;

    Ok(())
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::net::{SocketAddr, TcpListener as StdTcpListener};

    use axum::{Router, routing::get};
    use pyo3::types::{PyDict, PyList, PyTuple};
    use rstest::rstest;
    use tokio::net::TcpListener;

    use super::*;

    #[rstest]
    fn test_params_to_hashmap_none() {
        pyo3::Python::initialize();

        let result = Python::attach(|_py| params_to_hashmap(None)).unwrap();

        assert!(result.is_none());
    }

    #[rstest]
    fn test_params_to_hashmap_empty_dict() {
        pyo3::Python::initialize();

        let result = Python::attach(|py| {
            let dict = PyDict::new(py);
            params_to_hashmap(Some(dict.as_any()))
        })
        .unwrap();

        assert!(result.is_some());
        assert!(result.unwrap().is_empty());
    }

    #[rstest]
    fn test_params_to_hashmap_single_string_value() {
        pyo3::Python::initialize();

        let result = Python::attach(|py| {
            let dict = PyDict::new(py);
            dict.set_item("key", "value").unwrap();
            params_to_hashmap(Some(dict.as_any()))
        })
        .unwrap()
        .unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result.get("key").unwrap(), &vec!["value"]);
    }

    #[rstest]
    fn test_params_to_hashmap_multiple_string_values() {
        pyo3::Python::initialize();

        let result = Python::attach(|py| {
            let dict = PyDict::new(py);
            dict.set_item("foo", "bar").unwrap();
            dict.set_item("limit", "100").unwrap();
            dict.set_item("offset", "0").unwrap();
            params_to_hashmap(Some(dict.as_any()))
        })
        .unwrap()
        .unwrap();

        assert_eq!(result.len(), 3);
        assert_eq!(result.get("foo").unwrap(), &vec!["bar"]);
        assert_eq!(result.get("limit").unwrap(), &vec!["100"]);
        assert_eq!(result.get("offset").unwrap(), &vec!["0"]);
    }

    #[rstest]
    fn test_params_to_hashmap_int_value() {
        pyo3::Python::initialize();

        let result = Python::attach(|py| {
            let dict = PyDict::new(py);
            dict.set_item("limit", 100).unwrap();
            params_to_hashmap(Some(dict.as_any()))
        })
        .unwrap()
        .unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result.get("limit").unwrap(), &vec!["100"]);
    }

    #[rstest]
    fn test_params_to_hashmap_float_value() {
        pyo3::Python::initialize();

        let result = Python::attach(|py| {
            let dict = PyDict::new(py);
            dict.set_item("price", 123.45).unwrap();
            params_to_hashmap(Some(dict.as_any()))
        })
        .unwrap()
        .unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result.get("price").unwrap(), &vec!["123.45"]);
    }

    #[rstest]
    fn test_params_to_hashmap_bool_value() {
        pyo3::Python::initialize();

        let result = Python::attach(|py| {
            let dict = PyDict::new(py);
            dict.set_item("active", true).unwrap();
            dict.set_item("closed", false).unwrap();
            params_to_hashmap(Some(dict.as_any()))
        })
        .unwrap()
        .unwrap();

        assert_eq!(result.len(), 2);
        assert_eq!(result.get("active").unwrap(), &vec!["True"]);
        assert_eq!(result.get("closed").unwrap(), &vec!["False"]);
    }

    #[rstest]
    fn test_params_to_hashmap_list_value() {
        pyo3::Python::initialize();

        let result = Python::attach(|py| {
            let dict = PyDict::new(py);
            let list = PyList::new(py, ["1", "2", "3"]).unwrap();
            dict.set_item("id", list).unwrap();
            params_to_hashmap(Some(dict.as_any()))
        })
        .unwrap()
        .unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result.get("id").unwrap(), &vec!["1", "2", "3"]);
    }

    #[rstest]
    fn test_params_to_hashmap_tuple_value() {
        pyo3::Python::initialize();

        let result = Python::attach(|py| {
            let dict = PyDict::new(py);
            let tuple = PyTuple::new(py, ["a", "b", "c"]).unwrap();
            dict.set_item("letters", tuple).unwrap();
            params_to_hashmap(Some(dict.as_any()))
        })
        .unwrap()
        .unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result.get("letters").unwrap(), &vec!["a", "b", "c"]);
    }

    #[rstest]
    fn test_params_to_hashmap_list_with_mixed_types() {
        pyo3::Python::initialize();

        let result = Python::attach(|py| {
            let dict = PyDict::new(py);
            let list = PyList::new(py, [1, 2, 3]).unwrap();
            dict.set_item("nums", list).unwrap();
            params_to_hashmap(Some(dict.as_any()))
        })
        .unwrap()
        .unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result.get("nums").unwrap(), &vec!["1", "2", "3"]);
    }

    #[rstest]
    fn test_params_to_hashmap_mixed_values() {
        pyo3::Python::initialize();

        let result = Python::attach(|py| {
            let dict = PyDict::new(py);
            dict.set_item("name", "test").unwrap();
            dict.set_item("limit", 50).unwrap();
            let ids = PyList::new(py, ["1", "2"]).unwrap();
            dict.set_item("id", ids).unwrap();
            params_to_hashmap(Some(dict.as_any()))
        })
        .unwrap()
        .unwrap();

        assert_eq!(result.len(), 3);
        assert_eq!(result.get("name").unwrap(), &vec!["test"]);
        assert_eq!(result.get("limit").unwrap(), &vec!["50"]);
        assert_eq!(result.get("id").unwrap(), &vec!["1", "2"]);
    }

    #[rstest]
    fn test_params_to_hashmap_string_not_treated_as_sequence() {
        pyo3::Python::initialize();

        let result = Python::attach(|py| {
            let dict = PyDict::new(py);
            dict.set_item("text", "hello").unwrap();
            params_to_hashmap(Some(dict.as_any()))
        })
        .unwrap()
        .unwrap();

        assert_eq!(result.len(), 1);
        // String should be treated as single value, not as sequence of chars
        assert_eq!(result.get("text").unwrap(), &vec!["hello"]);
    }

    #[rstest]
    fn test_params_to_hashmap_invalid_non_dict() {
        pyo3::Python::initialize();

        let result = Python::attach(|py| {
            let list = PyList::new(py, ["a", "b"]).unwrap();
            params_to_hashmap(Some(list.as_any()))
        });

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("params must be a dict"));
    }

    #[rstest]
    fn test_params_to_hashmap_invalid_string_param() {
        pyo3::Python::initialize();

        let result = Python::attach(|py| {
            let string = pyo3::types::PyString::new(py, "not a dict");
            params_to_hashmap(Some(string.as_any()))
        });

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("params must be a dict"));
    }

    fn get_unique_port() -> u16 {
        let listener =
            StdTcpListener::bind("127.0.0.1:0").expect("Failed to bind temporary TcpListener");
        listener.local_addr().unwrap().port()
    }

    async fn create_test_router() -> Router {
        Router::new()
            .route("/get", get(|| async { "hello-world!" }))
            .route("/post", axum::routing::post(|| async { "posted" }))
            .route("/patch", axum::routing::patch(|| async { "patched" }))
            .route("/delete", axum::routing::delete(|| async { "deleted" }))
    }

    async fn start_test_server() -> Result<SocketAddr, Box<dyn std::error::Error + Send + Sync>> {
        let port = get_unique_port();
        let listener = TcpListener::bind(format!("127.0.0.1:{port}")).await?;
        let addr = listener.local_addr()?;

        tokio::spawn(async move {
            let app = create_test_router().await;
            axum::serve(listener, app).await.unwrap();
        });

        Ok(addr)
    }

    #[rstest]
    fn test_blocking_http_get() {
        pyo3::Python::initialize();

        let addr = get_runtime().block_on(async { start_test_server().await.unwrap() });
        let url = format!("http://{addr}/get");

        let response = Python::attach(|py| http_get(py, url, None, None, Some(10))).unwrap();

        assert!(response.status.is_success());
        assert_eq!(String::from_utf8_lossy(&response.body), "hello-world!");
    }

    #[rstest]
    fn test_blocking_http_post() {
        pyo3::Python::initialize();

        let addr = get_runtime().block_on(async { start_test_server().await.unwrap() });
        let url = format!("http://{addr}/post");

        let response = Python::attach(|py| http_post(py, url, None, None, None, Some(10))).unwrap();

        assert!(response.status.is_success());
        assert_eq!(String::from_utf8_lossy(&response.body), "posted");
    }

    #[rstest]
    fn test_blocking_http_patch() {
        pyo3::Python::initialize();

        let addr = get_runtime().block_on(async { start_test_server().await.unwrap() });
        let url = format!("http://{addr}/patch");

        let response =
            Python::attach(|py| http_patch(py, url, None, None, None, Some(10))).unwrap();

        assert!(response.status.is_success());
        assert_eq!(String::from_utf8_lossy(&response.body), "patched");
    }

    #[rstest]
    fn test_blocking_http_delete() {
        pyo3::Python::initialize();

        let addr = get_runtime().block_on(async { start_test_server().await.unwrap() });
        let url = format!("http://{addr}/delete");

        let response = Python::attach(|py| http_delete(py, url, None, None, Some(10))).unwrap();

        assert!(response.status.is_success());
        assert_eq!(String::from_utf8_lossy(&response.body), "deleted");
    }

    #[rstest]
    fn test_blocking_http_download() {
        pyo3::Python::initialize();

        let addr = get_runtime().block_on(async { start_test_server().await.unwrap() });
        let url = format!("http://{addr}/get");
        let temp_dir = std::env::temp_dir();
        let filepath = temp_dir.join("test_download.txt");

        Python::attach(|py| {
            http_download(
                py,
                url,
                filepath.to_str().unwrap().to_string(),
                None,
                None,
                Some(10),
            )
            .unwrap();
        });

        assert!(filepath.exists());
        let content = std::fs::read_to_string(&filepath).unwrap();
        assert_eq!(content, "hello-world!");

        std::fs::remove_file(&filepath).ok();
    }
}

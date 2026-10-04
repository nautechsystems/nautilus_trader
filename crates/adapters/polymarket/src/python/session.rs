// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

//! Thin Python bindings for owner-operated session administration.

use std::sync::Arc;

use nautilus_core::{python::to_pyvalue_err, string::secret::SecretString};
use pyo3::prelude::*;

use crate::session::{
    PolymarketSessionKey, PolymarketSessionKeyClient, PolymarketSessionKeyClientConfig,
};

#[pyclass(
    module = "nautilus_trader.adapters.polymarket",
    name = "PolymarketSessionKeyClientConfig",
    frozen,
    from_py_object
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.polymarket")]
#[derive(Debug, Clone)]
pub struct PyPolymarketSessionKeyClientConfig {
    inner: PolymarketSessionKeyClientConfig,
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyPolymarketSessionKeyClientConfig {
    #[new]
    #[expect(clippy::too_many_arguments)]
    #[pyo3(signature = (private_key, api_key, api_secret, passphrase, builder_api_key, builder_api_secret, builder_passphrase, funder, base_url_http=None, base_url_relayer=None, proxy_url=None))]
    fn py_new(
        private_key: String,
        api_key: String,
        api_secret: String,
        passphrase: String,
        builder_api_key: String,
        builder_api_secret: String,
        builder_passphrase: String,
        funder: String,
        base_url_http: Option<String>,
        base_url_relayer: Option<String>,
        proxy_url: Option<String>,
    ) -> Self {
        Self {
            inner: PolymarketSessionKeyClientConfig {
                private_key: private_key.into(),
                api_key: api_key.into(),
                api_secret: api_secret.into(),
                passphrase: passphrase.into(),
                builder_api_key: builder_api_key.into(),
                builder_api_secret: builder_api_secret.into(),
                builder_passphrase: builder_passphrase.into(),
                funder,
                base_url_http,
                base_url_relayer,
                proxy_url: proxy_url.map(SecretString::from),
            },
        }
    }

    #[getter]
    fn funder(&self) -> &str {
        &self.inner.funder
    }

    #[getter]
    fn base_url_http(&self) -> Option<&str> {
        self.inner.base_url_http.as_deref()
    }

    #[getter]
    fn base_url_relayer(&self) -> Option<&str> {
        self.inner.base_url_relayer.as_deref()
    }

    #[getter]
    fn has_proxy_url(&self) -> bool {
        self.inner.proxy_url.is_some()
    }

    fn __repr__(&self) -> String {
        format!("{:?}", self.inner)
    }
}

#[pyclass(
    module = "nautilus_trader.adapters.polymarket",
    name = "PolymarketSessionKey",
    frozen,
    skip_from_py_object
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.polymarket")]
#[derive(Debug, Clone)]
pub struct PyPolymarketSessionKey {
    inner: PolymarketSessionKey,
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyPolymarketSessionKey {
    #[getter]
    fn address(&self) -> &str {
        &self.inner.address
    }

    #[getter]
    fn scopes(&self) -> Vec<String> {
        self.inner.scopes.clone()
    }

    #[getter]
    fn valid_until(&self) -> u64 {
        self.inner.valid_until
    }

    fn __repr__(&self) -> String {
        format!("{:?}", self.inner)
    }
}

#[pyclass(
    module = "nautilus_trader.adapters.polymarket",
    name = "PolymarketSessionKeyClient",
    skip_from_py_object
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.polymarket")]
#[derive(Debug)]
pub struct PyPolymarketSessionKeyClient {
    inner: Arc<PolymarketSessionKeyClient>,
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyPolymarketSessionKeyClient {
    #[new]
    fn py_new(config: PyPolymarketSessionKeyClientConfig) -> PyResult<Self> {
        Ok(Self {
            inner: Arc::new(PolymarketSessionKeyClient::new(config.inner).map_err(to_pyvalue_err)?),
        })
    }

    fn list_session_keys<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            Ok(inner
                .list_session_keys()
                .await
                .map_err(to_pyvalue_err)?
                .into_iter()
                .map(|inner| PyPolymarketSessionKey { inner })
                .collect::<Vec<_>>())
        })
    }

    fn authorize_session_key<'py>(
        &self,
        py: Python<'py>,
        address: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            Ok(PyPolymarketSessionKey {
                inner: inner
                    .authorize_session_key(&address)
                    .await
                    .map_err(to_pyvalue_err)?,
            })
        })
    }

    fn revoke_session_key<'py>(
        &self,
        py: Python<'py>,
        address: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            inner
                .revoke_session_key(&address)
                .await
                .map_err(to_pyvalue_err)
        })
    }
}

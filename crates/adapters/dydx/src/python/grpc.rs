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

//! Python bindings for dYdX gRPC client.

#![allow(clippy::missing_errors_doc)]

use std::sync::Arc;

use nautilus_core::python::{IntoPyObjectNautilusExt, to_pyruntime_err};
use pyo3::prelude::*;

use crate::grpc::DydxGrpcClient;

#[pyclass(name = "DydxGrpcClient", from_py_object)]
#[derive(Debug, Clone)]
pub struct PyDydxGrpcClient {
    pub(crate) inner: Arc<DydxGrpcClient>,
}

#[pymethods]
impl PyDydxGrpcClient {
    /// Create a new gRPC client.
    ///
    /// # Errors
    ///
    /// Returns an error if connection fails.
    #[staticmethod]
    #[pyo3(name = "connect")]
    pub fn py_connect(py: Python<'_>, grpc_url: String) -> PyResult<Bound<'_, PyAny>> {
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let client = DydxGrpcClient::new(grpc_url)
                .await
                .map_err(to_pyruntime_err)?;

            Ok(Self {
                inner: Arc::new(client),
            })
        })
    }

    /// Create a new gRPC client with fallback URLs.
    ///
    /// # Errors
    ///
    /// Returns an error if all connection attempts fail.
    #[staticmethod]
    #[pyo3(name = "connect_with_fallback")]
    pub fn py_connect_with_fallback(
        py: Python<'_>,
        grpc_urls: Vec<String>,
    ) -> PyResult<Bound<'_, PyAny>> {
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let urls: Vec<&str> = grpc_urls.iter().map(String::as_str).collect();
            let client = DydxGrpcClient::new_with_fallback(&urls)
                .await
                .map_err(to_pyruntime_err)?;

            Ok(Self {
                inner: Arc::new(client),
            })
        })
    }

    /// Fetch the latest block height from the chain.
    ///
    /// # Errors
    ///
    /// Returns an error if the gRPC request fails.
    #[pyo3(name = "latest_block_height")]
    pub fn py_latest_block_height<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut client = (*client).clone();
            let height = client
                .latest_block_height()
                .await
                .map_err(to_pyruntime_err)?;
            Ok(height.0 as u64)
        })
    }

    /// Query account information (account_number, sequence).
    ///
    /// # Errors
    ///
    /// Returns an error if the gRPC request fails.
    #[pyo3(name = "get_account")]
    pub fn py_get_account<'py>(
        &self,
        py: Python<'py>,
        address: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut client = (*client).clone();
            let account = client
                .get_account(&address)
                .await
                .map_err(to_pyruntime_err)?;
            Ok((account.account_number, account.sequence))
        })
    }

    /// Query account balances.
    ///
    /// # Errors
    ///
    /// Returns an error if the gRPC request fails.
    #[pyo3(name = "get_account_balances")]
    pub fn py_get_account_balances<'py>(
        &self,
        py: Python<'py>,
        address: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut client = (*client).clone();
            let balances = client
                .get_account_balances(&address)
                .await
                .map_err(to_pyruntime_err)?;
            let result: Vec<(String, String)> =
                balances.into_iter().map(|c| (c.denom, c.amount)).collect();
            Ok(result)
        })
    }

    /// Query subaccount information.
    ///
    /// # Errors
    ///
    /// Returns an error if the gRPC request fails.
    #[pyo3(name = "get_subaccount")]
    pub fn py_get_subaccount<'py>(
        &self,
        py: Python<'py>,
        address: String,
        subaccount_number: u32,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut client = (*client).clone();
            let subaccount = client
                .get_subaccount(&address, subaccount_number)
                .await
                .map_err(to_pyruntime_err)?;

            // Return as dict-like structure
            // quantums is bytes representing a big-endian signed integer
            let result: Vec<(String, String)> = subaccount
                .asset_positions
                .into_iter()
                .map(|p| {
                    let quantums_str = if p.quantums.is_empty() {
                        "0".to_string()
                    } else {
                        // Convert bytes to hex string for now
                        hex::encode(&p.quantums)
                    };
                    (p.asset_id.to_string(), quantums_str)
                })
                .collect();
            Ok(result)
        })
    }

    /// Get node information from the gRPC endpoint.
    ///
    /// # Errors
    ///
    /// Returns an error if the gRPC request fails.
    #[pyo3(name = "get_node_info")]
    pub fn py_get_node_info<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut client = (*client).clone();
            let info = client.get_node_info().await.map_err(to_pyruntime_err)?;

            // Return node info as a dict
            Python::attach(|py| {
                use pyo3::types::PyDict;
                let dict = PyDict::new(py);

                if let Some(default_node_info) = info.default_node_info {
                    dict.set_item("network", default_node_info.network)?;
                    dict.set_item("moniker", default_node_info.moniker)?;
                    dict.set_item("version", default_node_info.version)?;
                }

                if let Some(app_info) = info.application_version {
                    dict.set_item("app_name", app_info.name)?;
                    dict.set_item("app_version", app_info.version)?;
                }
                Ok(dict.into_py_any_unwrap(py))
            })
        })
    }

    /// Simulate a transaction to estimate gas.
    ///
    /// # Errors
    ///
    /// Returns an error if the gRPC request fails.
    #[pyo3(name = "simulate_tx")]
    pub fn py_simulate_tx<'py>(
        &self,
        py: Python<'py>,
        tx_bytes: Vec<u8>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut client = (*client).clone();
            let gas_used = client
                .simulate_tx(tx_bytes)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(gas_used)
        })
    }

    /// Get transaction details by hash.
    ///
    /// # Errors
    ///
    /// Returns an error if the gRPC request fails.
    #[pyo3(name = "get_tx")]
    pub fn py_get_tx<'py>(&self, py: Python<'py>, hash: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut client = (*client).clone();
            let tx = client.get_tx(&hash).await.map_err(to_pyruntime_err)?;

            // Return tx as JSON string
            let result = format!("Tx(body_bytes_len={})", tx.body.messages.len());
            Ok(result)
        })
    }

    fn __repr__(&self) -> String {
        "DydxGrpcClient()".to_string()
    }
}

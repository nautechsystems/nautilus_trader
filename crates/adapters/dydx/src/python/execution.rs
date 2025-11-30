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

//! Python bindings for dYdX execution components.

use std::{str::FromStr, sync::Arc};

use pyo3::prelude::*;

use crate::{
    execution::submitter::OrderSubmitter,
    grpc::{DydxGrpcClient, Wallet, types::ChainId},
    http::client::DydxHttpClient,
};

/// Python wrapper for the Wallet.
#[pyclass(name = "DydxWallet")]
#[derive(Debug, Clone)]
pub struct PyDydxWallet {
    pub(crate) inner: Arc<Wallet>,
}

#[pymethods]
impl PyDydxWallet {
    /// Create a wallet from a 24-word English mnemonic phrase.
    ///
    /// # Errors
    ///
    /// Returns an error if the mnemonic is invalid.
    #[staticmethod]
    #[pyo3(name = "from_mnemonic")]
    pub fn py_from_mnemonic(mnemonic: &str) -> PyResult<Self> {
        let wallet = Wallet::from_mnemonic(mnemonic)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(format!("{e}")))?;
        Ok(Self {
            inner: Arc::new(wallet),
        })
    }

    fn __repr__(&self) -> String {
        "DydxWallet(<redacted>)".to_string()
    }
}

/// Python wrapper for OrderSubmitter.
#[pyclass(name = "DydxOrderSubmitter")]
#[derive(Debug)]
pub struct PyDydxOrderSubmitter {
    pub(crate) inner: Arc<OrderSubmitter>,
}

#[pymethods]
impl PyDydxOrderSubmitter {
    /// Create a new order submitter.
    ///
    /// # Errors
    ///
    /// Returns an error if the chain ID is invalid.
    #[new]
    #[pyo3(signature = (grpc_client, http_client, wallet_address, subaccount_number=0, chain_id=None))]
    pub fn py_new(
        grpc_client: PyDydxGrpcClient,
        http_client: DydxHttpClient,
        wallet_address: String,
        subaccount_number: u32,
        chain_id: Option<&str>,
    ) -> PyResult<Self> {
        let chain_id = if let Some(chain_str) = chain_id {
            ChainId::from_str(chain_str)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(format!("{e}")))?
        } else {
            ChainId::Mainnet1
        };

        let submitter = OrderSubmitter::new(
            grpc_client.inner.as_ref().clone(),
            http_client,
            wallet_address,
            subaccount_number,
            chain_id,
        );

        Ok(Self {
            inner: Arc::new(submitter),
        })
    }

    /// Submit a market order to dYdX via gRPC.
    #[pyo3(name = "submit_market_order")]
    #[allow(clippy::too_many_arguments)]
    fn py_submit_market_order<'py>(
        &self,
        py: Python<'py>,
        wallet: PyDydxWallet,
        instrument_id: &str,
        client_order_id: u32,
        side: i64,
        quantity: &str,
        block_height: u32,
    ) -> PyResult<Bound<'py, PyAny>> {
        let submitter = self.inner.clone();
        let wallet_inner = wallet.inner;
        let instrument_id = nautilus_model::identifiers::InstrumentId::from(instrument_id);
        let side = nautilus_model::enums::OrderSide::from_repr(side as usize)
            .ok_or_else(|| PyErr::new::<pyo3::exceptions::PyValueError, _>("Invalid OrderSide"))?;
        let quantity = nautilus_model::types::Quantity::from(quantity);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            submitter
                .submit_market_order(
                    &wallet_inner,
                    instrument_id,
                    client_order_id,
                    side,
                    quantity,
                    block_height,
                )
                .await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{e}")))?;
            Ok(())
        })
    }

    /// Submit a limit order to dYdX via gRPC.
    #[pyo3(name = "submit_limit_order")]
    #[allow(clippy::too_many_arguments)]
    fn py_submit_limit_order<'py>(
        &self,
        py: Python<'py>,
        wallet: PyDydxWallet,
        instrument_id: &str,
        client_order_id: u32,
        side: i64,
        price: &str,
        quantity: &str,
        time_in_force: i64,
        post_only: bool,
        reduce_only: bool,
        block_height: u32,
        expire_time: Option<i64>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let submitter = self.inner.clone();
        let wallet_inner = wallet.inner;
        let instrument_id = nautilus_model::identifiers::InstrumentId::from(instrument_id);
        let side = nautilus_model::enums::OrderSide::from_repr(side as usize)
            .ok_or_else(|| PyErr::new::<pyo3::exceptions::PyValueError, _>("Invalid OrderSide"))?;
        let price = nautilus_model::types::Price::from(price);
        let quantity = nautilus_model::types::Quantity::from(quantity);
        let time_in_force = nautilus_model::enums::TimeInForce::from_repr(time_in_force as usize)
            .ok_or_else(|| {
            PyErr::new::<pyo3::exceptions::PyValueError, _>("Invalid TimeInForce")
        })?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            submitter
                .submit_limit_order(
                    &wallet_inner,
                    instrument_id,
                    client_order_id,
                    side,
                    price,
                    quantity,
                    time_in_force,
                    post_only,
                    reduce_only,
                    block_height,
                    expire_time,
                )
                .await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{e}")))?;
            Ok(())
        })
    }

    /// Cancel an order on dYdX via gRPC.
    #[pyo3(name = "cancel_order")]
    fn py_cancel_order<'py>(
        &self,
        py: Python<'py>,
        wallet: PyDydxWallet,
        instrument_id: &str,
        client_order_id: u32,
        block_height: u32,
    ) -> PyResult<Bound<'py, PyAny>> {
        let submitter = self.inner.clone();
        let wallet_inner = wallet.inner;
        let instrument_id = nautilus_model::identifiers::InstrumentId::from(instrument_id);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            submitter
                .cancel_order(&wallet_inner, instrument_id, client_order_id, block_height)
                .await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{e}")))?;
            Ok(())
        })
    }

    fn __repr__(&self) -> String {
        "DydxOrderSubmitter()".to_string()
    }
}

/// Python wrapper for DydxGrpcClient.
#[pyclass(name = "DydxGrpcClient")]
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
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{e}")))?;

            Ok(Self {
                inner: Arc::new(client),
            })
        })
    }

    /// Create a new gRPC client with fallback URLs.
    ///
    /// # Errors
    ///
    /// Returns an error if all connections fail.
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
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{e}")))?;

            Ok(Self {
                inner: Arc::new(client),
            })
        })
    }

    /// Fetch the latest block height from the chain.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    #[pyo3(name = "latest_block_height")]
    pub fn py_latest_block_height<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut client = (*client).clone();
            let height = client
                .latest_block_height()
                .await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{e}")))?;
            Ok(height.0 as u64)
        })
    }

    fn __repr__(&self) -> String {
        "DydxGrpcClient()".to_string()
    }
}

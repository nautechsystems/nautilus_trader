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

use nautilus_core::python::IntoPyObjectNautilusExt;
use nautilus_model::{
    enums::{OrderSide, TimeInForce},
    identifiers::InstrumentId,
    types::{Price, Quantity},
};
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

    /// Get the wallet address (derives from account index 0).
    ///
    /// # Errors
    ///
    /// Returns an error if address derivation fails.
    #[pyo3(name = "address")]
    pub fn py_address(&self) -> PyResult<String> {
        let account = self
            .inner
            .account_offline(0)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{e}")))?;
        Ok(account.address)
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
    /// Returns an error if chain_id is invalid.
    #[new]
    #[pyo3(signature = (grpc_client, http_client, wallet_address, subaccount_number=0, chain_id=None, authenticator_ids=None))]
    pub fn py_new(
        grpc_client: PyDydxGrpcClient,
        http_client: DydxHttpClient,
        wallet_address: String,
        subaccount_number: u32,
        chain_id: Option<&str>,
        authenticator_ids: Option<Vec<u64>>,
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
            authenticator_ids.unwrap_or_default(),
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
        let instrument_id = InstrumentId::from(instrument_id);
        let side = OrderSide::from_repr(side as usize)
            .ok_or_else(|| PyErr::new::<pyo3::exceptions::PyValueError, _>("Invalid OrderSide"))?;
        let quantity = Quantity::from(quantity);

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
        let instrument_id = InstrumentId::from(instrument_id);
        let side = OrderSide::from_repr(side as usize)
            .ok_or_else(|| PyErr::new::<pyo3::exceptions::PyValueError, _>("Invalid OrderSide"))?;
        let price = Price::from(price);
        let quantity = Quantity::from(quantity);
        let time_in_force = TimeInForce::from_repr(time_in_force as usize).ok_or_else(|| {
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

    /// Submit a stop market order to dYdX via gRPC.
    #[pyo3(name = "submit_stop_market_order")]
    #[allow(clippy::too_many_arguments)]
    fn py_submit_stop_market_order<'py>(
        &self,
        py: Python<'py>,
        wallet: PyDydxWallet,
        instrument_id: &str,
        client_order_id: u32,
        side: i64,
        trigger_price: &str,
        quantity: &str,
        reduce_only: bool,
        expire_time: Option<i64>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let submitter = self.inner.clone();
        let wallet_inner = wallet.inner;
        let instrument_id = InstrumentId::from(instrument_id);
        let side = OrderSide::from_repr(side as usize)
            .ok_or_else(|| PyErr::new::<pyo3::exceptions::PyValueError, _>("Invalid OrderSide"))?;
        let trigger_price = Price::from(trigger_price);
        let quantity = Quantity::from(quantity);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            submitter
                .submit_stop_market_order(
                    &wallet_inner,
                    instrument_id,
                    client_order_id,
                    side,
                    trigger_price,
                    quantity,
                    reduce_only,
                    expire_time,
                )
                .await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{e}")))?;
            Ok(())
        })
    }

    /// Submit a stop limit order to dYdX via gRPC.
    #[pyo3(name = "submit_stop_limit_order")]
    #[allow(clippy::too_many_arguments)]
    fn py_submit_stop_limit_order<'py>(
        &self,
        py: Python<'py>,
        wallet: PyDydxWallet,
        instrument_id: &str,
        client_order_id: u32,
        side: i64,
        trigger_price: &str,
        limit_price: &str,
        quantity: &str,
        time_in_force: i64,
        post_only: bool,
        reduce_only: bool,
        expire_time: Option<i64>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let submitter = self.inner.clone();
        let wallet_inner = wallet.inner;
        let instrument_id = InstrumentId::from(instrument_id);
        let side = OrderSide::from_repr(side as usize)
            .ok_or_else(|| PyErr::new::<pyo3::exceptions::PyValueError, _>("Invalid OrderSide"))?;
        let trigger_price = Price::from(trigger_price);
        let limit_price = Price::from(limit_price);
        let quantity = Quantity::from(quantity);
        let time_in_force = TimeInForce::from_repr(time_in_force as usize).ok_or_else(|| {
            PyErr::new::<pyo3::exceptions::PyValueError, _>("Invalid TimeInForce")
        })?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            submitter
                .submit_stop_limit_order(
                    &wallet_inner,
                    instrument_id,
                    client_order_id,
                    side,
                    trigger_price,
                    limit_price,
                    quantity,
                    time_in_force,
                    post_only,
                    reduce_only,
                    expire_time,
                )
                .await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{e}")))?;
            Ok(())
        })
    }

    /// Submit a take profit market order to dYdX via gRPC.
    #[pyo3(name = "submit_take_profit_market_order")]
    #[allow(clippy::too_many_arguments)]
    fn py_submit_take_profit_market_order<'py>(
        &self,
        py: Python<'py>,
        wallet: PyDydxWallet,
        instrument_id: &str,
        client_order_id: u32,
        side: i64,
        trigger_price: &str,
        quantity: &str,
        reduce_only: bool,
        expire_time: Option<i64>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let submitter = self.inner.clone();
        let wallet_inner = wallet.inner;
        let instrument_id = InstrumentId::from(instrument_id);
        let side = OrderSide::from_repr(side as usize)
            .ok_or_else(|| PyErr::new::<pyo3::exceptions::PyValueError, _>("Invalid OrderSide"))?;
        let trigger_price = Price::from(trigger_price);
        let quantity = Quantity::from(quantity);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            submitter
                .submit_take_profit_market_order(
                    &wallet_inner,
                    instrument_id,
                    client_order_id,
                    side,
                    trigger_price,
                    quantity,
                    reduce_only,
                    expire_time,
                )
                .await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{e}")))?;
            Ok(())
        })
    }

    /// Submit a take profit limit order to dYdX via gRPC.
    #[pyo3(name = "submit_take_profit_limit_order")]
    #[allow(clippy::too_many_arguments)]
    fn py_submit_take_profit_limit_order<'py>(
        &self,
        py: Python<'py>,
        wallet: PyDydxWallet,
        instrument_id: &str,
        client_order_id: u32,
        side: i64,
        trigger_price: &str,
        limit_price: &str,
        quantity: &str,
        time_in_force: i64,
        post_only: bool,
        reduce_only: bool,
        expire_time: Option<i64>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let submitter = self.inner.clone();
        let wallet_inner = wallet.inner;
        let instrument_id = InstrumentId::from(instrument_id);
        let side = OrderSide::from_repr(side as usize)
            .ok_or_else(|| PyErr::new::<pyo3::exceptions::PyValueError, _>("Invalid OrderSide"))?;
        let trigger_price = Price::from(trigger_price);
        let limit_price = Price::from(limit_price);
        let quantity = Quantity::from(quantity);
        let time_in_force = TimeInForce::from_repr(time_in_force as usize).ok_or_else(|| {
            PyErr::new::<pyo3::exceptions::PyValueError, _>("Invalid TimeInForce")
        })?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            submitter
                .submit_take_profit_limit_order(
                    &wallet_inner,
                    instrument_id,
                    client_order_id,
                    side,
                    trigger_price,
                    limit_price,
                    quantity,
                    time_in_force,
                    post_only,
                    reduce_only,
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
        let instrument_id = InstrumentId::from(instrument_id);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            submitter
                .cancel_order(&wallet_inner, instrument_id, client_order_id, block_height)
                .await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{e}")))?;
            Ok(())
        })
    }

    /// Cancel multiple orders on dYdX via gRPC.
    #[pyo3(name = "cancel_orders_batch")]
    fn py_cancel_orders_batch<'py>(
        &self,
        py: Python<'py>,
        wallet: PyDydxWallet,
        orders: Vec<(String, u32)>,
        block_height: u32,
    ) -> PyResult<Bound<'py, PyAny>> {
        let submitter = self.inner.clone();
        let wallet_inner = wallet.inner;
        let orders: Vec<(InstrumentId, u32)> = orders
            .into_iter()
            .map(|(id, client_id)| (InstrumentId::from(id.as_str()), client_id))
            .collect();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            submitter
                .cancel_orders_batch(&wallet_inner, &orders, block_height)
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
    /// Returns an error if the gRPC request fails.
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
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{e}")))?;
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
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{e}")))?;
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
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{e}")))?;

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
            let info = client
                .get_node_info()
                .await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{e}")))?;

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
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{e}")))?;
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
            let tx = client
                .get_tx(&hash)
                .await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{e}")))?;

            // Return tx as JSON string
            let result = format!("Tx(body_bytes_len={})", tx.body.messages.len());
            Ok(result)
        })
    }

    fn __repr__(&self) -> String {
        "DydxGrpcClient()".to_string()
    }
}

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

//! Python bindings for dYdX order submitter.

use std::{num::NonZeroU32, str::FromStr, sync::Arc};

use chrono::Utc;
use nautilus_core::{
    UnixNanos,
    python::{to_pyruntime_err, to_pyvalue_err},
};
use nautilus_model::{
    enums::{OrderSide, TimeInForce},
    identifiers::InstrumentId,
    types::{Price, Quantity},
};
use nautilus_network::ratelimiter::quota::Quota;
use pyo3::prelude::*;

use super::grpc::PyDydxGrpcClient;
use crate::{
    execution::{block_time::BlockTimeMonitor, submitter::OrderSubmitter},
    grpc::{DEFAULT_RUST_CLIENT_METADATA, types::ChainId},
    http::client::DydxHttpClient,
};

/// Python wrapper for OrderSubmitter.
///
/// # Breaking Change
///
/// This class now takes `private_key` in the constructor instead of requiring
/// a wallet to be passed to each method. The wallet is owned internally.
///
/// ```python
/// # Before (old API):
/// wallet = DydxWallet.from_private_key("...")
/// submitter = DydxOrderSubmitter(grpc, http, address, ...)
/// submitter.submit_market_order(wallet, instrument_id, ...)
///
/// # After (new API):
/// submitter = DydxOrderSubmitter(grpc, http, private_key="...", ...)
/// submitter.submit_market_order(instrument_id, ...)  # no wallet param
/// ```
#[pyclass(name = "DydxOrderSubmitter")]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.dydx")]
#[derive(Debug)]
pub struct PyDydxOrderSubmitter {
    pub(crate) inner: Arc<OrderSubmitter>,
    /// Block time monitor - updated via `record_block()`.
    block_time_monitor: Arc<BlockTimeMonitor>,
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyDydxOrderSubmitter {
    /// Create a new order submitter with wallet owned internally.
    ///
    /// # Arguments
    ///
    /// * `grpc_client` - gRPC client for chain operations
    /// * `http_client` - HTTP client (provides market params cache)
    /// * `private_key` - Private key (hex-encoded) for signing transactions
    /// * `wallet_address` - Main account address (may differ from derived address for permissioned keys)
    /// * `subaccount_number` - dYdX subaccount number (default: 0)
    /// * `chain_id` - Chain ID string (default: "dydx-mainnet-1")
    /// * `grpc_rate_limit_per_second` - Optional gRPC rate limit (requests per second)
    ///
    /// # Errors
    ///
    /// Returns an error if chain_id is invalid or wallet creation fails.
    #[new]
    #[pyo3(signature = (
        grpc_client,
        http_client,
        private_key,
        wallet_address,
        subaccount_number=0,
        chain_id=None,
        grpc_rate_limit_per_second=None,
    ))]
    #[expect(clippy::needless_pass_by_value)]
    pub fn py_new(
        grpc_client: PyDydxGrpcClient,
        http_client: DydxHttpClient,
        private_key: &str,
        wallet_address: String,
        subaccount_number: u32,
        chain_id: Option<&str>,
        grpc_rate_limit_per_second: Option<u32>,
    ) -> PyResult<Self> {
        let chain_id = if let Some(chain_str) = chain_id {
            ChainId::from_str(chain_str).map_err(to_pyvalue_err)?
        } else {
            ChainId::Mainnet1
        };

        let grpc_quota = grpc_rate_limit_per_second
            .and_then(NonZeroU32::new)
            .and_then(Quota::per_second);

        // Create block time monitor (updated via record_block)
        let block_time_monitor = Arc::new(BlockTimeMonitor::new());

        let submitter = OrderSubmitter::new(
            grpc_client.inner.as_ref().clone(),
            http_client,
            private_key,
            wallet_address,
            subaccount_number,
            chain_id,
            Arc::clone(&block_time_monitor),
            grpc_quota,
        )
        .map_err(to_pyvalue_err)?;

        Ok(Self {
            inner: Arc::new(submitter),
            block_time_monitor,
        })
    }

    /// Record a block height update with timestamp.
    ///
    /// Call this when receiving block updates from WebSocket.
    /// The timestamp should be the block's timestamp (ISO 8601 format).
    ///
    /// # Errors
    ///
    /// Returns an error if the timestamp cannot be parsed.
    #[pyo3(name = "record_block")]
    fn py_record_block(&self, height: u64, timestamp: Option<&str>) -> PyResult<()> {
        let time = if let Some(ts) = timestamp {
            chrono::DateTime::parse_from_rfc3339(ts)
                .map(|dt| dt.with_timezone(&Utc))
                .map_err(|e| to_pyvalue_err(format!("Invalid timestamp: {e}")))?
        } else {
            Utc::now()
        };
        self.block_time_monitor.record_block(height, time);
        Ok(())
    }

    /// Set the current block height (legacy API, uses current time).
    ///
    /// Prefer using `record_block` with actual block timestamp for accurate
    /// block time estimation.
    #[pyo3(name = "set_block_height")]
    fn py_set_block_height(&self, height: u64) {
        self.block_time_monitor.record_block(height, Utc::now());
    }

    /// Get the current block height.
    #[pyo3(name = "get_block_height")]
    fn py_get_block_height(&self) -> u64 {
        self.block_time_monitor.current_block_height()
    }

    /// Get the estimated seconds per block (based on rolling average).
    ///
    /// Returns None if insufficient samples have been collected.
    #[pyo3(name = "estimated_seconds_per_block")]
    fn py_estimated_seconds_per_block(&self) -> Option<f64> {
        self.block_time_monitor.estimated_seconds_per_block()
    }

    /// Check if the block time monitor has enough samples for reliable estimates.
    #[pyo3(name = "is_block_time_ready")]
    fn py_is_block_time_ready(&self) -> bool {
        self.block_time_monitor.is_ready()
    }

    /// Get the wallet address.
    #[pyo3(name = "wallet_address")]
    fn py_wallet_address(&self) -> String {
        self.inner.wallet_address().to_string()
    }

    /// Resolve authenticator IDs for permissioned key trading.
    ///
    /// Call this during connect() when using an API trading key.
    /// Automatically detects if the signing wallet differs from the main account
    /// and fetches matching authenticators from the chain.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Using permissioned key but no authenticators found
    /// - No authenticator matches the wallet's public key
    /// - gRPC query fails
    #[pyo3(name = "resolve_authenticators")]
    fn py_resolve_authenticators<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let submitter = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            submitter
                .tx_manager()
                .resolve_authenticators()
                .await
                .map_err(to_pyruntime_err)
        })
    }

    /// Submit a market order to dYdX via gRPC.
    ///
    /// Block height is read from the internal state (set via `set_block_height`).
    #[pyo3(name = "submit_market_order")]
    #[pyo3(signature = (instrument_id, client_order_id, side, quantity, client_metadata=None))]
    fn py_submit_market_order<'py>(
        &self,
        py: Python<'py>,
        instrument_id: &str,
        client_order_id: u32,
        side: i64,
        quantity: &str,
        client_metadata: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let submitter = self.inner.clone();
        let instrument_id = InstrumentId::from(instrument_id);
        let side = OrderSide::from_repr(side as usize)
            .ok_or_else(|| to_pyvalue_err("Invalid OrderSide"))?;
        let quantity = Quantity::from(quantity);
        let client_metadata = client_metadata.unwrap_or(DEFAULT_RUST_CLIENT_METADATA);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let tx_hash = submitter
                .submit_market_order(
                    instrument_id,
                    client_order_id,
                    client_metadata,
                    side,
                    quantity,
                )
                .await
                .map_err(to_pyruntime_err)?;
            Ok(tx_hash)
        })
    }

    /// Submit a limit order to dYdX via gRPC.
    ///
    /// Block height is read from the internal state (set via `set_block_height`).
    #[pyo3(name = "submit_limit_order")]
    #[pyo3(signature = (instrument_id, client_order_id, side, price, quantity, time_in_force, post_only, reduce_only, expire_time=None, client_metadata=None))]
    #[expect(clippy::too_many_arguments)]
    fn py_submit_limit_order<'py>(
        &self,
        py: Python<'py>,
        instrument_id: &str,
        client_order_id: u32,
        side: i64,
        price: &str,
        quantity: &str,
        time_in_force: i64,
        post_only: bool,
        reduce_only: bool,
        expire_time: Option<i64>,
        client_metadata: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let submitter = self.inner.clone();
        let instrument_id = InstrumentId::from(instrument_id);
        let side = OrderSide::from_repr(side as usize)
            .ok_or_else(|| to_pyvalue_err("Invalid OrderSide"))?;
        let price = Price::from(price);
        let quantity = Quantity::from(quantity);
        let time_in_force = TimeInForce::from_repr(time_in_force as usize)
            .ok_or_else(|| to_pyvalue_err("Invalid TimeInForce"))?;
        let client_metadata = client_metadata.unwrap_or(DEFAULT_RUST_CLIENT_METADATA);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let tx_hash = submitter
                .submit_limit_order(
                    instrument_id,
                    client_order_id,
                    client_metadata,
                    side,
                    price,
                    quantity,
                    time_in_force,
                    post_only,
                    reduce_only,
                    expire_time,
                )
                .await
                .map_err(to_pyruntime_err)?;
            Ok(tx_hash)
        })
    }

    /// Submit a stop market order to dYdX via gRPC.
    #[pyo3(name = "submit_stop_market_order")]
    #[pyo3(signature = (instrument_id, client_order_id, side, trigger_price, quantity, reduce_only, expire_time=None, client_metadata=None))]
    #[expect(clippy::too_many_arguments)]
    fn py_submit_stop_market_order<'py>(
        &self,
        py: Python<'py>,
        instrument_id: &str,
        client_order_id: u32,
        side: i64,
        trigger_price: &str,
        quantity: &str,
        reduce_only: bool,
        expire_time: Option<i64>,
        client_metadata: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let submitter = self.inner.clone();
        let instrument_id = InstrumentId::from(instrument_id);
        let side = OrderSide::from_repr(side as usize)
            .ok_or_else(|| to_pyvalue_err("Invalid OrderSide"))?;
        let trigger_price = Price::from(trigger_price);
        let quantity = Quantity::from(quantity);
        let client_metadata = client_metadata.unwrap_or(DEFAULT_RUST_CLIENT_METADATA);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let tx_hash = submitter
                .submit_stop_market_order(
                    instrument_id,
                    client_order_id,
                    client_metadata,
                    side,
                    trigger_price,
                    quantity,
                    reduce_only,
                    expire_time,
                )
                .await
                .map_err(to_pyruntime_err)?;
            Ok(tx_hash)
        })
    }

    /// Submit a stop limit order to dYdX via gRPC.
    #[pyo3(name = "submit_stop_limit_order")]
    #[pyo3(signature = (instrument_id, client_order_id, side, trigger_price, limit_price, quantity, time_in_force, post_only, reduce_only, expire_time=None, client_metadata=None))]
    #[expect(clippy::too_many_arguments)]
    fn py_submit_stop_limit_order<'py>(
        &self,
        py: Python<'py>,
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
        client_metadata: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let submitter = self.inner.clone();
        let instrument_id = InstrumentId::from(instrument_id);
        let side = OrderSide::from_repr(side as usize)
            .ok_or_else(|| to_pyvalue_err("Invalid OrderSide"))?;
        let trigger_price = Price::from(trigger_price);
        let limit_price = Price::from(limit_price);
        let quantity = Quantity::from(quantity);
        let time_in_force = TimeInForce::from_repr(time_in_force as usize)
            .ok_or_else(|| to_pyvalue_err("Invalid TimeInForce"))?;
        let client_metadata = client_metadata.unwrap_or(DEFAULT_RUST_CLIENT_METADATA);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let tx_hash = submitter
                .submit_stop_limit_order(
                    instrument_id,
                    client_order_id,
                    client_metadata,
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
                .map_err(to_pyruntime_err)?;
            Ok(tx_hash)
        })
    }

    /// Submit a take profit market order to dYdX via gRPC.
    #[pyo3(name = "submit_take_profit_market_order")]
    #[pyo3(signature = (instrument_id, client_order_id, side, trigger_price, quantity, reduce_only, expire_time=None, client_metadata=None))]
    #[expect(clippy::too_many_arguments)]
    fn py_submit_take_profit_market_order<'py>(
        &self,
        py: Python<'py>,
        instrument_id: &str,
        client_order_id: u32,
        side: i64,
        trigger_price: &str,
        quantity: &str,
        reduce_only: bool,
        expire_time: Option<i64>,
        client_metadata: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let submitter = self.inner.clone();
        let instrument_id = InstrumentId::from(instrument_id);
        let side = OrderSide::from_repr(side as usize)
            .ok_or_else(|| to_pyvalue_err("Invalid OrderSide"))?;
        let trigger_price = Price::from(trigger_price);
        let quantity = Quantity::from(quantity);
        let client_metadata = client_metadata.unwrap_or(DEFAULT_RUST_CLIENT_METADATA);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let tx_hash = submitter
                .submit_take_profit_market_order(
                    instrument_id,
                    client_order_id,
                    client_metadata,
                    side,
                    trigger_price,
                    quantity,
                    reduce_only,
                    expire_time,
                )
                .await
                .map_err(to_pyruntime_err)?;
            Ok(tx_hash)
        })
    }

    /// Submit a take profit limit order to dYdX via gRPC.
    #[pyo3(name = "submit_take_profit_limit_order")]
    #[pyo3(signature = (instrument_id, client_order_id, side, trigger_price, limit_price, quantity, time_in_force, post_only, reduce_only, expire_time=None, client_metadata=None))]
    #[expect(clippy::too_many_arguments)]
    fn py_submit_take_profit_limit_order<'py>(
        &self,
        py: Python<'py>,
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
        client_metadata: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let submitter = self.inner.clone();
        let instrument_id = InstrumentId::from(instrument_id);
        let side = OrderSide::from_repr(side as usize)
            .ok_or_else(|| to_pyvalue_err("Invalid OrderSide"))?;
        let trigger_price = Price::from(trigger_price);
        let limit_price = Price::from(limit_price);
        let quantity = Quantity::from(quantity);
        let time_in_force = TimeInForce::from_repr(time_in_force as usize)
            .ok_or_else(|| to_pyvalue_err("Invalid TimeInForce"))?;
        let client_metadata = client_metadata.unwrap_or(DEFAULT_RUST_CLIENT_METADATA);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let tx_hash = submitter
                .submit_take_profit_limit_order(
                    instrument_id,
                    client_order_id,
                    client_metadata,
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
                .map_err(to_pyruntime_err)?;
            Ok(tx_hash)
        })
    }

    /// Cancel an order on dYdX.
    ///
    /// Block height is read from the internal state (set via `set_block_height`).
    #[pyo3(name = "cancel_order")]
    #[pyo3(signature = (instrument_id, client_order_id, time_in_force=None, expire_time_ns=None))]
    fn py_cancel_order<'py>(
        &self,
        py: Python<'py>,
        instrument_id: &str,
        client_order_id: u32,
        time_in_force: Option<i64>,
        expire_time_ns: Option<u64>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let submitter = self.inner.clone();
        let instrument_id = InstrumentId::from(instrument_id);
        let time_in_force = time_in_force
            .and_then(|tif| TimeInForce::from_repr(tif as usize))
            .unwrap_or(TimeInForce::Gtc);
        let expire_time_ns = expire_time_ns.map(UnixNanos::from);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let tx_hash = submitter
                .cancel_order(
                    instrument_id,
                    client_order_id,
                    time_in_force,
                    expire_time_ns,
                )
                .await
                .map_err(to_pyruntime_err)?;
            Ok(tx_hash)
        })
    }

    /// Cancel multiple orders in a single transaction.
    ///
    /// Each order is specified as (instrument_id, client_order_id, time_in_force, expire_time_ns).
    /// For simplified usage, time_in_force and expire_time_ns can be omitted (defaults to GTC).
    #[pyo3(name = "cancel_orders_batch")]
    fn py_cancel_orders_batch<'py>(
        &self,
        py: Python<'py>,
        orders: Vec<(String, u32, Option<i64>, Option<u64>)>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let submitter = self.inner.clone();
        let orders: Vec<(InstrumentId, u32, TimeInForce, Option<UnixNanos>)> = orders
            .into_iter()
            .map(|(id, client_id, tif, expire_ns)| {
                let tif = tif
                    .and_then(|t| TimeInForce::from_repr(t as usize))
                    .unwrap_or(TimeInForce::Gtc);
                let expire_ns = expire_ns.map(UnixNanos::from);
                (InstrumentId::from(id), client_id, tif, expire_ns)
            })
            .collect();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let tx_hash = submitter
                .cancel_orders_batch(&orders)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(tx_hash)
        })
    }

    fn __repr__(&self) -> String {
        format!(
            "DydxOrderSubmitter(address={}, block_height={})",
            self.inner.wallet_address(),
            self.block_time_monitor.current_block_height()
        )
    }
}

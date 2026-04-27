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

//! Python bindings for the BitMEX submit broadcaster.

use nautilus_core::python::to_pyvalue_err;
use nautilus_model::{
    enums::{ContingencyType, OrderSide, OrderType, TimeInForce, TrailingOffsetType, TriggerType},
    identifiers::{ClientOrderId, InstrumentId, OrderListId},
    python::instruments::pyobject_to_instrument_any,
    types::{Price, Quantity},
};
use pyo3::{conversion::IntoPyObjectExt, prelude::*, types::PyDict};

use crate::{
    broadcast::submitter::{SubmitBroadcaster, SubmitBroadcasterConfig},
    common::enums::{BitmexEnvironment, BitmexPegPriceType},
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl SubmitBroadcaster {
    /// Broadcasts submit requests to multiple HTTP clients for redundancy.
    ///
    /// This broadcaster fans out submit requests to multiple pre-warmed HTTP clients
    /// in parallel, short-circuits when the first successful acknowledgement is received,
    /// and handles expected rejection patterns (duplicate clOrdID) with appropriate log levels.
    #[new]
    #[pyo3(signature = (
        pool_size,
        api_key=None,
        api_secret=None,
        base_url=None,
        environment=BitmexEnvironment::Mainnet,
        timeout_secs=60,
        max_retries=3,
        retry_delay_ms=1_000,
        retry_delay_max_ms=5_000,
        recv_window_ms=10_000,
        max_requests_per_second=10,
        max_requests_per_minute=120,
        health_check_interval_secs=30,
        health_check_timeout_secs=5,
        expected_reject_patterns=None,
        proxy_urls=None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        pool_size: usize,
        api_key: Option<String>,
        api_secret: Option<String>,
        base_url: Option<String>,
        environment: BitmexEnvironment,
        timeout_secs: u64,
        max_retries: u32,
        retry_delay_ms: u64,
        retry_delay_max_ms: u64,
        recv_window_ms: u64,
        max_requests_per_second: u32,
        max_requests_per_minute: u32,
        health_check_interval_secs: u64,
        health_check_timeout_secs: u64,
        expected_reject_patterns: Option<Vec<String>>,
        proxy_urls: Option<Vec<Option<String>>>,
    ) -> PyResult<Self> {
        let config = SubmitBroadcasterConfig {
            pool_size,
            api_key,
            api_secret,
            base_url,
            environment,
            timeout_secs,
            max_retries,
            retry_delay_ms,
            retry_delay_max_ms,
            recv_window_ms,
            max_requests_per_second,
            max_requests_per_minute,
            health_check_interval_secs,
            health_check_timeout_secs,
            expected_reject_patterns: expected_reject_patterns
                .unwrap_or_else(|| SubmitBroadcasterConfig::default().expected_reject_patterns),
            proxy_urls: proxy_urls.unwrap_or_default(),
        };

        Self::new(config).map_err(to_pyvalue_err)
    }

    /// Starts the broadcaster and health check loop.
    ///
    /// # Errors
    ///
    /// Returns an error if the broadcaster is already running.
    #[pyo3(name = "start")]
    fn py_start<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let broadcaster = self.clone_for_async();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            broadcaster.start().await.map_err(to_pyvalue_err)
        })
    }

    /// Stops the broadcaster and health check loop.
    #[pyo3(name = "stop")]
    fn py_stop<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let broadcaster = self.clone_for_async();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            broadcaster.stop().await;
            Ok(())
        })
    }

    /// Broadcasts a submit request to all healthy clients in parallel.
    ///
    /// # Returns
    ///
    /// - `Ok(report)` if successfully submitted with a report.
    /// - `Err` if all requests failed.
    #[pyo3(name = "broadcast_submit")]
    #[pyo3(signature = (
        instrument_id,
        client_order_id,
        order_side,
        order_type,
        quantity,
        time_in_force,
        price=None,
        trigger_price=None,
        trigger_type=None,
        trailing_offset=None,
        trailing_offset_type=None,
        display_qty=None,
        post_only=false,
        reduce_only=false,
        order_list_id=None,
        contingency_type=None,
        submit_tries=None,
        peg_price_type=None,
        peg_offset_value=None
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_broadcast_submit<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        time_in_force: TimeInForce,
        price: Option<Price>,
        trigger_price: Option<Price>,
        trigger_type: Option<TriggerType>,
        trailing_offset: Option<f64>,
        trailing_offset_type: Option<TrailingOffsetType>,
        display_qty: Option<Quantity>,
        post_only: bool,
        reduce_only: bool,
        order_list_id: Option<OrderListId>,
        contingency_type: Option<ContingencyType>,
        submit_tries: Option<usize>,
        peg_price_type: Option<String>,
        peg_offset_value: Option<f64>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let broadcaster = self.clone_for_async();

        let peg_price_type: Option<BitmexPegPriceType> = peg_price_type
            .map(|s| {
                s.parse::<BitmexPegPriceType>()
                    .map_err(|_| to_pyvalue_err(format!("Invalid peg_price_type: {s}")))
            })
            .transpose()?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let report = broadcaster
                .broadcast_submit(
                    instrument_id,
                    client_order_id,
                    order_side,
                    order_type,
                    quantity,
                    time_in_force,
                    price,
                    trigger_price,
                    trigger_type,
                    trailing_offset,
                    trailing_offset_type,
                    display_qty,
                    post_only,
                    reduce_only,
                    order_list_id,
                    contingency_type,
                    submit_tries,
                    peg_price_type,
                    peg_offset_value,
                )
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| report.into_py_any(py))
        })
    }

    /// Gets broadcaster metrics.
    #[pyo3(name = "get_metrics")]
    fn py_get_metrics(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let metrics = self.get_metrics();
        let dict = PyDict::new(py);
        dict.set_item("total_submits", metrics.total_submits)?;
        dict.set_item("successful_submits", metrics.successful_submits)?;
        dict.set_item("failed_submits", metrics.failed_submits)?;
        dict.set_item("expected_rejects", metrics.expected_rejects)?;
        dict.set_item("healthy_clients", metrics.healthy_clients)?;
        dict.set_item("total_clients", metrics.total_clients)?;
        Ok(dict.into())
    }

    /// Gets per-client statistics.
    #[pyo3(name = "get_client_stats")]
    fn py_get_client_stats(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let stats = self.get_client_stats();
        let list = pyo3::types::PyList::empty(py);
        for stat in stats {
            let dict = PyDict::new(py);
            dict.set_item("client_id", stat.client_id.clone())?;
            dict.set_item("healthy", stat.healthy)?;
            dict.set_item("submit_count", stat.submit_count)?;
            dict.set_item("error_count", stat.error_count)?;
            list.append(dict)?;
        }
        Ok(list.into())
    }

    /// Caches an instrument in all HTTP clients in the pool.
    #[pyo3(name = "cache_instrument")]
    fn py_cache_instrument(&self, py: Python, instrument: Py<PyAny>) -> PyResult<()> {
        let inst_any = pyobject_to_instrument_any(py, instrument)?;
        self.cache_instrument(&inst_any);
        Ok(())
    }
}

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

//! Python bindings for the BitMEX submit broadcaster.

use nautilus_core::python::to_pyvalue_err;
use nautilus_model::{
    enums::{ContingencyType, OrderSide, OrderType, TimeInForce, TriggerType},
    identifiers::{ClientOrderId, InstrumentId, OrderListId},
    python::instruments::pyobject_to_instrument_any,
    types::{Price, Quantity},
};
use pyo3::{conversion::IntoPyObjectExt, prelude::*, types::PyDict};

use crate::execution::submitter::{SubmitBroadcaster, SubmitBroadcasterConfig};

#[pymethods]
impl SubmitBroadcaster {
    #[new]
    #[pyo3(signature = (
        pool_size,
        api_key=None,
        api_secret=None,
        base_url=None,
        testnet=false,
        timeout_secs=None,
        max_retries=None,
        retry_delay_ms=None,
        retry_delay_max_ms=None,
        recv_window_ms=None,
        max_requests_per_second=None,
        max_requests_per_minute=None,
        health_check_interval_secs=30,
        health_check_timeout_secs=5,
        expected_reject_patterns=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        pool_size: usize,
        api_key: Option<String>,
        api_secret: Option<String>,
        base_url: Option<String>,
        testnet: bool,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        recv_window_ms: Option<u64>,
        max_requests_per_second: Option<u32>,
        max_requests_per_minute: Option<u32>,
        health_check_interval_secs: u64,
        health_check_timeout_secs: u64,
        expected_reject_patterns: Option<Vec<String>>,
    ) -> PyResult<Self> {
        let config = SubmitBroadcasterConfig {
            pool_size,
            api_key,
            api_secret,
            base_url,
            testnet,
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
            proxy_urls: vec![], // TODO: Add proxy_urls parameter to Python API when needed
        };

        Self::new(config).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "start")]
    fn py_start<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let broadcaster = self.clone_for_async();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            broadcaster.start().await.map_err(to_pyvalue_err)
        })
    }

    #[pyo3(name = "stop")]
    fn py_stop<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let broadcaster = self.clone_for_async();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            broadcaster.stop().await;
            Ok(())
        })
    }

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
        display_qty=None,
        post_only=false,
        reduce_only=false,
        order_list_id=None,
        contingency_type=None,
        submit_tries=None
    ))]
    #[allow(clippy::too_many_arguments)]
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
        display_qty: Option<Quantity>,
        post_only: bool,
        reduce_only: bool,
        order_list_id: Option<OrderListId>,
        contingency_type: Option<ContingencyType>,
        submit_tries: Option<usize>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let broadcaster = self.clone_for_async();
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
                    display_qty,
                    post_only,
                    reduce_only,
                    order_list_id,
                    contingency_type,
                    submit_tries,
                )
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| report.into_py_any(py))
        })
    }

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

    #[pyo3(name = "cache_instrument")]
    fn py_cache_instrument(&self, py: Python, instrument: Py<PyAny>) -> PyResult<()> {
        let inst_any = pyobject_to_instrument_any(py, instrument)?;
        self.cache_instrument(inst_any);
        Ok(())
    }
}

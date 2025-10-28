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

//! Python bindings for the BitMEX cancel broadcaster.

use nautilus_core::python::to_pyvalue_err;
use nautilus_model::{
    identifiers::{ClientOrderId, InstrumentId, VenueOrderId},
    python::instruments::pyobject_to_instrument_any,
};
use pyo3::{conversion::IntoPyObjectExt, prelude::*, types::PyDict};

use crate::execution::canceller::{CancelBroadcaster, CancelBroadcasterConfig};

#[pymethods]
impl CancelBroadcaster {
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
        expected_reject_patterns=None,
        idempotent_success_patterns=None
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
        idempotent_success_patterns: Option<Vec<String>>,
    ) -> PyResult<Self> {
        let config = CancelBroadcasterConfig {
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
                .unwrap_or_else(|| CancelBroadcasterConfig::default().expected_reject_patterns),
            idempotent_success_patterns: idempotent_success_patterns
                .unwrap_or_else(|| CancelBroadcasterConfig::default().idempotent_success_patterns),
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

    #[pyo3(name = "broadcast_cancel")]
    fn py_broadcast_cancel<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let broadcaster = self.clone_for_async();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let report = broadcaster
                .broadcast_cancel(instrument_id, client_order_id, venue_order_id)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| match report {
                Some(r) => r.into_py_any(py),
                None => Ok(py.None()),
            })
        })
    }

    #[pyo3(name = "broadcast_batch_cancel")]
    fn py_broadcast_batch_cancel<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        client_order_ids: Option<Vec<ClientOrderId>>,
        venue_order_ids: Option<Vec<VenueOrderId>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let broadcaster = self.clone_for_async();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let reports = broadcaster
                .broadcast_batch_cancel(instrument_id, client_order_ids, venue_order_ids)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let py_reports: PyResult<Vec<_>> = reports
                    .into_iter()
                    .map(|report| report.into_py_any(py))
                    .collect();
                let pylist = pyo3::types::PyList::new(py, py_reports?)
                    .unwrap()
                    .into_any()
                    .unbind();
                Ok(pylist)
            })
        })
    }

    #[pyo3(name = "broadcast_cancel_all")]
    fn py_broadcast_cancel_all<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        order_side: Option<nautilus_model::enums::OrderSide>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let broadcaster = self.clone_for_async();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let reports = broadcaster
                .broadcast_cancel_all(instrument_id, order_side)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let py_reports: PyResult<Vec<_>> = reports
                    .into_iter()
                    .map(|report| report.into_py_any(py))
                    .collect();
                let pylist = pyo3::types::PyList::new(py, py_reports?)
                    .unwrap()
                    .into_any()
                    .unbind();
                Ok(pylist)
            })
        })
    }

    #[pyo3(name = "get_metrics")]
    fn py_get_metrics(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let metrics = self.get_metrics();
        let dict = PyDict::new(py);
        dict.set_item("total_cancels", metrics.total_cancels)?;
        dict.set_item("successful_cancels", metrics.successful_cancels)?;
        dict.set_item("failed_cancels", metrics.failed_cancels)?;
        dict.set_item("expected_rejects", metrics.expected_rejects)?;
        dict.set_item("idempotent_successes", metrics.idempotent_successes)?;
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
            dict.set_item("cancel_count", stat.cancel_count)?;
            dict.set_item("error_count", stat.error_count)?;
            list.append(dict)?;
        }
        Ok(list.into())
    }

    #[pyo3(name = "add_instrument")]
    fn py_add_instrument(&self, py: Python, instrument: Py<PyAny>) -> PyResult<()> {
        let inst_any = pyobject_to_instrument_any(py, instrument)?;
        self.add_instrument(inst_any);
        Ok(())
    }
}

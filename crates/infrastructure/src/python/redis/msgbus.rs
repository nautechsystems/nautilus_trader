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

use bytes::Bytes;
use futures::{pin_mut, stream::StreamExt};
use nautilus_common::msgbus::database::MessageBusDatabaseAdapter;
use nautilus_core::{
    UUID4,
    python::{IntoPyObjectNautilusExt, to_pyruntime_err, to_pyvalue_err},
};
use nautilus_model::identifiers::TraderId;
use pyo3::prelude::*;
use ustr::Ustr;

use crate::redis::msgbus::RedisMessageBusDatabase;

#[pymethods]
impl RedisMessageBusDatabase {
    #[new]
    fn py_new(trader_id: TraderId, instance_id: UUID4, config_json: Vec<u8>) -> PyResult<Self> {
        let config = serde_json::from_slice(&config_json).map_err(to_pyvalue_err)?;
        Self::new(trader_id, instance_id, config).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "is_closed")]
    fn py_is_closed(&self) -> bool {
        self.is_closed()
    }

    #[pyo3(name = "publish")]
    fn py_publish(&self, topic: String, payload: Vec<u8>) {
        self.publish(Ustr::from(&topic), Bytes::from(payload));
    }

    #[pyo3(name = "stream")]
    fn py_stream<'py>(
        &mut self,
        callback: Py<PyAny>,
        py: Python<'py>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let stream_rx = self.get_stream_receiver().map_err(to_pyruntime_err)?;
        let stream = Self::stream(stream_rx);
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            pin_mut!(stream);
            while let Some(msg) = stream.next().await {
                Python::attach(|py| call_python(py, &callback, msg.into_py_any_unwrap(py)));
            }
            Ok(())
        })
    }

    #[pyo3(name = "close")]
    fn py_close(&mut self) {
        self.close();
    }
}

fn call_python(py: Python, callback: &Py<PyAny>, py_obj: Py<PyAny>) {
    if let Err(e) = callback.call1(py, (py_obj,)) {
        tracing::error!("Error calling Python: {e}");
    }
}

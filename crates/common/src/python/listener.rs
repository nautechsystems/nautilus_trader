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
use futures::pin_mut;
use nautilus_core::python::{IntoPyObjectNautilusExt, to_pyruntime_err};
use pyo3::prelude::*;

use crate::msgbus::listener::MessageBusListener;

#[pymethods]
impl MessageBusListener {
    #[new]
    fn py_new() -> PyResult<Self> {
        Ok(Self::new())
    }

    #[pyo3(name = "is_active")]
    fn py_is_active(&self) -> bool {
        !self.is_closed()
    }

    #[pyo3(name = "is_closed")]
    fn py_is_closed(&self) -> bool {
        self.is_closed()
    }

    #[pyo3(name = "close")]
    fn py_close(&mut self) {
        self.close()
    }

    #[pyo3(name = "publish")]
    fn py_publish(&self, topic: String, payload: Vec<u8>) {
        self.publish(topic, Bytes::from(payload))
    }

    #[pyo3(name = "stream")]
    fn py_stream<'py>(
        &mut self,
        callback: PyObject,
        py: Python<'py>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let stream_rx = self.get_stream_receiver().map_err(to_pyruntime_err)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            pin_mut!(stream_rx);
            while let Some(msg) = stream_rx.recv().await {
                Python::with_gil(|py| call_python(py, &callback, msg.into_py_any_unwrap(py)));
            }
            Ok(())
        })
    }
}

fn call_python(py: Python, callback: &PyObject, py_obj: PyObject) {
    if let Err(e) = callback.call1(py, (py_obj,)) {
        tracing::error!("Error calling Python: {e}");
    }
}

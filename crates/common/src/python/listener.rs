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

#![cfg(feature = "live")]

use bytes::Bytes;
use futures::pin_mut;
use nautilus_core::python::{IntoPyObjectNautilusExt, call_python, to_pyruntime_err};
use pyo3::prelude::*;
use ustr::Ustr;

use crate::live::listener::MessageBusListener;

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl MessageBusListener {
    /// Creates a new `MessageBusListener` instance.
    #[new]
    fn py_new() -> Self {
        Self::new()
    }

    #[pyo3(name = "is_active")]
    fn py_is_active(&self) -> bool {
        !self.is_closed()
    }

    /// Returns whether the listener is closed.
    #[pyo3(name = "is_closed")]
    fn py_is_closed(&self) -> bool {
        self.is_closed()
    }

    /// Closes the listener.
    #[pyo3(name = "close")]
    fn py_close(&mut self) {
        self.close();
    }

    /// Publishes a message with the given `topic` and `payload`.
    #[pyo3(name = "publish")]
    fn py_publish(&self, topic: &str, payload: Vec<u8>) {
        self.publish(Ustr::from(topic), Bytes::from(payload));
    }

    /// Streams messages arriving on the receiver channel.
    #[pyo3(name = "stream")]
    fn py_stream<'py>(
        &mut self,
        callback: Py<PyAny>,
        py: Python<'py>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let stream_rx = self.get_stream_receiver().map_err(to_pyruntime_err)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            pin_mut!(stream_rx);
            while let Some(msg) = stream_rx.recv().await {
                Python::attach(|py| call_python(py, &callback, msg.into_py_any_unwrap(py)));
            }
            Ok(())
        })
    }
}

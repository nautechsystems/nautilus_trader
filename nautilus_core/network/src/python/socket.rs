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

use std::sync::{atomic::Ordering, Arc};

use nautilus_core::python::to_pyruntime_err;
use pyo3::prelude::*;
use tokio::io::AsyncWriteExt;
use tokio_tungstenite::tungstenite::stream::Mode;

use crate::socket::{SocketClient, SocketConfig};

#[pymethods]
impl SocketConfig {
    #[new]
    #[pyo3(signature = (url, ssl, suffix, handler, heartbeat=None, max_reconnection_tries=3))]
    fn py_new(
        url: String,
        ssl: bool,
        suffix: Vec<u8>,
        handler: PyObject,
        heartbeat: Option<(u64, Vec<u8>)>,
        max_reconnection_tries: Option<u64>,
    ) -> Self {
        let mode = if ssl { Mode::Tls } else { Mode::Plain };
        Self {
            url,
            mode,
            suffix,
            handler: Arc::new(handler),
            heartbeat,
            max_reconnection_tries,
        }
    }
}

#[pymethods]
impl SocketClient {
    /// Create a socket client.
    ///
    /// # Errors
    ///
    /// - Throws an Exception if it is unable to make socket connection.
    #[staticmethod]
    #[pyo3(name = "connect")]
    #[pyo3(signature = (config, post_connection=None, post_reconnection=None, post_disconnection=None))]
    fn py_connect(
        config: SocketConfig,
        post_connection: Option<PyObject>,
        post_reconnection: Option<PyObject>,
        post_disconnection: Option<PyObject>,
        py: Python<'_>,
    ) -> PyResult<Bound<PyAny>> {
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            Self::connect(
                config,
                post_connection,
                post_reconnection,
                post_disconnection,
            )
            .await
            .map_err(to_pyruntime_err)
        })
    }

    /// Closes the client heart beat and reader task.
    ///
    /// The connection is not completely closed until all references
    /// to the client are gone and the client is dropped.
    ///
    /// # Safety
    ///
    /// - The client should not be used after closing it
    /// - Any auto-reconnect job should be aborted before closing the client
    #[pyo3(name = "disconnect")]
    fn py_disconnect<'py>(slf: PyRef<'_, Self>, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let disconnect_mode = slf.disconnect_mode.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            disconnect_mode.store(true, Ordering::SeqCst);
            Ok(())
        })
    }

    /// Check if the client is still alive.
    ///
    /// Even if the connection is disconnected the client will still be alive
    /// and try to reconnect. Only when reconnect fails the client will
    /// terminate.
    ///
    /// This is particularly useful for check why a `send` failed. It could
    /// be because the connection disconnected and the client is still alive
    /// and reconnecting. In such cases the send can be retried after some
    /// delay
    #[pyo3(name = "is_alive")]
    fn py_is_alive(slf: PyRef<'_, Self>) -> bool {
        !slf.controller_task.is_finished()
    }

    /// Send bytes data to the connection.
    ///
    /// # Errors
    ///
    /// - Throws an Exception if it is not able to send data.
    #[pyo3(name = "send")]
    fn py_send<'py>(
        slf: PyRef<'_, Self>,
        mut data: Vec<u8>,
        py: Python<'py>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let writer = slf.writer.clone();
        data.extend(&slf.suffix);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut writer = writer.lock().await;
            writer.write_all(&data).await?;
            Ok(())
        })
    }
}

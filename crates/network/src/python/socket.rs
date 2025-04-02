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

use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

use nautilus_core::python::to_pyruntime_err;
use pyo3::prelude::*;
use tokio_tungstenite::tungstenite::stream::Mode;

use crate::{
    mode::ConnectionMode,
    socket::{SocketClient, SocketConfig, WriterCommand},
};

#[pymethods]
impl SocketConfig {
    #[new]
    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (url, ssl, suffix, handler, heartbeat=None, reconnect_timeout_ms=10_000, reconnect_delay_initial_ms=2_000, reconnect_delay_max_ms=30_000, reconnect_backoff_factor=1.5, reconnect_jitter_ms=100, certs_dir=None))]
    fn py_new(
        url: String,
        ssl: bool,
        suffix: Vec<u8>,
        handler: PyObject,
        heartbeat: Option<(u64, Vec<u8>)>,
        reconnect_timeout_ms: Option<u64>,
        reconnect_delay_initial_ms: Option<u64>,
        reconnect_delay_max_ms: Option<u64>,
        reconnect_backoff_factor: Option<f64>,
        reconnect_jitter_ms: Option<u64>,
        certs_dir: Option<String>,
    ) -> Self {
        let mode = if ssl { Mode::Tls } else { Mode::Plain };
        Self {
            url,
            mode,
            suffix,
            py_handler: Some(Arc::new(handler)),
            heartbeat,
            reconnect_timeout_ms,
            reconnect_delay_initial_ms,
            reconnect_delay_max_ms,
            reconnect_backoff_factor,
            reconnect_jitter_ms,
            certs_dir,
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
                None, // Rust handler
                post_connection,
                post_reconnection,
                post_disconnection,
            )
            .await
            .map_err(to_pyruntime_err)
        })
    }

    /// Check if the client is still alive.
    ///
    /// Even if the connection is disconnected the client will still be alive
    /// and trying to reconnect.
    ///
    /// This is particularly useful for check why a `send` failed. It could
    /// be because the connection disconnected and the client is still alive
    /// and reconnecting. In such cases the send can be retried after some
    /// delay
    #[pyo3(name = "is_active")]
    fn py_is_active(slf: PyRef<'_, Self>) -> bool {
        slf.is_active()
    }

    #[pyo3(name = "is_reconnecting")]
    fn py_is_reconnecting(slf: PyRef<'_, Self>) -> bool {
        slf.is_reconnecting()
    }

    #[pyo3(name = "is_disconnecting")]
    fn py_is_disconnecting(slf: PyRef<'_, Self>) -> bool {
        slf.is_disconnecting()
    }

    #[pyo3(name = "is_closed")]
    fn py_is_closed(slf: PyRef<'_, Self>) -> bool {
        slf.is_closed()
    }

    #[pyo3(name = "mode")]
    fn py_mode(slf: PyRef<'_, Self>) -> String {
        slf.connection_mode().to_string()
    }

    /// Reconnect the client.
    #[pyo3(name = "reconnect")]
    fn py_reconnect<'py>(slf: PyRef<'_, Self>, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let mode = slf.connection_mode.clone();
        let mode_str = ConnectionMode::from_atomic(&mode).to_string();
        tracing::debug!("Reconnect from mode {mode_str}");

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            match ConnectionMode::from_atomic(&mode) {
                ConnectionMode::Reconnect => {
                    tracing::warn!("Cannot reconnect - socket already reconnecting");
                }
                ConnectionMode::Disconnect => {
                    tracing::warn!("Cannot reconnect - socket disconnecting");
                }
                ConnectionMode::Closed => {
                    tracing::warn!("Cannot reconnect - socket closed");
                }
                _ => {
                    mode.store(ConnectionMode::Reconnect.as_u8(), Ordering::SeqCst);
                    while !ConnectionMode::from_atomic(&mode).is_active() {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                }
            }

            Ok(())
        })
    }

    /// Close the client.
    ///
    /// The connection is not completely closed until all references
    /// to the client are gone and the client is dropped.
    ///
    /// # Safety
    ///
    /// - The client should not be used after closing it
    /// - Any auto-reconnect job should be aborted before closing the client
    #[pyo3(name = "close")]
    fn py_close<'py>(slf: PyRef<'_, Self>, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let mode = slf.connection_mode.clone();
        let mode_str = ConnectionMode::from_atomic(&mode).to_string();
        tracing::debug!("Close from mode {mode_str}");

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            match ConnectionMode::from_atomic(&mode) {
                ConnectionMode::Closed => {
                    tracing::warn!("Socket already closed");
                }
                ConnectionMode::Disconnect => {
                    tracing::warn!("Socket already disconnecting");
                }
                _ => {
                    mode.store(ConnectionMode::Disconnect.as_u8(), Ordering::SeqCst);
                    while !ConnectionMode::from_atomic(&mode).is_closed() {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                }
            }

            Ok(())
        })
    }

    /// Send bytes data to the connection.
    ///
    /// # Errors
    ///
    /// - Throws an Exception if it is not able to send data.
    #[pyo3(name = "send")]
    fn py_send<'py>(
        slf: PyRef<'_, Self>,
        data: Vec<u8>,
        py: Python<'py>,
    ) -> PyResult<Bound<'py, PyAny>> {
        tracing::trace!("Sending {}", String::from_utf8_lossy(&data));

        let mode = slf.connection_mode.clone();
        let writer_tx = slf.writer_tx.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if ConnectionMode::from_atomic(&mode).is_closed() {
                let msg = format!(
                    "Cannot send data ({}): socket closed",
                    String::from_utf8_lossy(&data)
                );
                log::error!("{msg}");
                return Ok(());
            }

            let timeout = Duration::from_secs(2);
            let check_interval = Duration::from_millis(1);

            if !ConnectionMode::from_atomic(&mode).is_active() {
                tracing::debug!("Waiting for client to become ACTIVE before sending (2s)...");
                match tokio::time::timeout(timeout, async {
                    while !ConnectionMode::from_atomic(&mode).is_active() {
                        if matches!(
                            ConnectionMode::from_atomic(&mode),
                            ConnectionMode::Disconnect | ConnectionMode::Closed
                        ) {
                            return Err("Client disconnected waiting to send");
                        }

                        tokio::time::sleep(check_interval).await;
                    }

                    Ok(())
                })
                .await
                {
                    Ok(Ok(())) => tracing::debug!("Client now active"),
                    Ok(Err(e)) => {
                        tracing::error!(
                            "Failed sending data ({}): {e}",
                            String::from_utf8_lossy(&data)
                        );
                        return Ok(());
                    }
                    Err(_) => {
                        tracing::error!(
                            "Failed sending data ({}): timeout waiting to become ACTIVE",
                            String::from_utf8_lossy(&data)
                        );
                        return Ok(());
                    }
                }
            }

            let msg = WriterCommand::Send(data.into());
            if let Err(e) = writer_tx.send(msg) {
                tracing::error!("{e}");
            }
            Ok(())
        })
    }
}

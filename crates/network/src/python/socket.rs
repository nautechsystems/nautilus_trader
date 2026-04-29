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

use std::{sync::atomic::Ordering, time::Duration};

use nautilus_core::python::{clone_py_object, to_pyruntime_err};
use pyo3::{Py, prelude::*};
use tokio_tungstenite::tungstenite::stream::Mode;

use crate::{
    mode::ConnectionMode,
    socket::{SocketClient, SocketConfig, TcpMessageHandler, WriterCommand},
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl SocketConfig {
    /// Configuration for TCP socket connection.
    #[new]
    #[expect(clippy::too_many_arguments, clippy::needless_pass_by_value)]
    #[pyo3(signature = (url, ssl, suffix, handler, heartbeat=None, reconnect_timeout_ms=10_000, reconnect_delay_initial_ms=2_000, reconnect_delay_max_ms=30_000, reconnect_backoff_factor=1.5, reconnect_jitter_ms=100, connection_max_retries=5, reconnect_max_attempts=None, idle_timeout_ms=None, certs_dir=None))]
    fn py_new(
        url: String,
        ssl: bool,
        suffix: Vec<u8>,
        handler: Py<PyAny>,
        heartbeat: Option<(u64, Vec<u8>)>,
        reconnect_timeout_ms: Option<u64>,
        reconnect_delay_initial_ms: Option<u64>,
        reconnect_delay_max_ms: Option<u64>,
        reconnect_backoff_factor: Option<f64>,
        reconnect_jitter_ms: Option<u64>,
        connection_max_retries: Option<u32>,
        reconnect_max_attempts: Option<u32>,
        idle_timeout_ms: Option<u64>,
        certs_dir: Option<String>,
    ) -> Self {
        let mode = if ssl { Mode::Tls } else { Mode::Plain };

        // Create function pointer that calls Python handler
        let handler_clone = clone_py_object(&handler);
        let message_handler: TcpMessageHandler = std::sync::Arc::new(move |data: &[u8]| {
            Python::attach(|py| {
                if let Err(e) = handler_clone.call1(py, (data,)) {
                    log::error!("Error calling Python message handler: {e}");
                }
            });
        });

        Self {
            url,
            mode,
            suffix,
            message_handler: Some(message_handler),
            heartbeat,
            reconnect_timeout_ms,
            reconnect_delay_initial_ms,
            reconnect_delay_max_ms,
            reconnect_backoff_factor,
            reconnect_jitter_ms,
            connection_max_retries,
            reconnect_max_attempts,
            idle_timeout_ms,
            certs_dir,
        }
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl SocketClient {
    /// Connect to the server.
    #[staticmethod]
    #[pyo3(name = "connect")]
    #[pyo3(signature = (config, post_connection=None, post_reconnection=None, post_disconnection=None))]
    fn py_connect(
        config: SocketConfig,
        post_connection: Option<Py<PyAny>>,
        post_reconnection: Option<Py<PyAny>>,
        post_disconnection: Option<Py<PyAny>>,
        py: Python<'_>,
    ) -> PyResult<Bound<'_, PyAny>> {
        // Convert Python callbacks to function pointers
        let post_connection_fn = post_connection.map(|callback| {
            let callback_clone = clone_py_object(&callback);
            std::sync::Arc::new(move || {
                Python::attach(|py| {
                    if let Err(e) = callback_clone.call0(py) {
                        log::error!("Error calling post_connection handler: {e}");
                    }
                });
            }) as std::sync::Arc<dyn Fn() + Send + Sync>
        });

        let post_reconnection_fn = post_reconnection.map(|callback| {
            let callback_clone = clone_py_object(&callback);
            std::sync::Arc::new(move || {
                Python::attach(|py| {
                    if let Err(e) = callback_clone.call0(py) {
                        log::error!("Error calling post_reconnection handler: {e}");
                    }
                });
            }) as std::sync::Arc<dyn Fn() + Send + Sync>
        });

        let post_disconnection_fn = post_disconnection.map(|callback| {
            let callback_clone = clone_py_object(&callback);
            std::sync::Arc::new(move || {
                Python::attach(|py| {
                    if let Err(e) = callback_clone.call0(py) {
                        log::error!("Error calling post_disconnection handler: {e}");
                    }
                });
            }) as std::sync::Arc<dyn Fn() + Send + Sync>
        });

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            Self::connect(
                config,
                post_connection_fn,
                post_reconnection_fn,
                post_disconnection_fn,
            )
            .await
            .map_err(to_pyruntime_err)
        })
    }

    /// Check if the client connection is active.
    ///
    /// Returns `true` if the client is connected and has not been signalled to disconnect.
    /// The client will automatically retry connection based on its configuration.
    #[pyo3(name = "is_active")]
    #[expect(clippy::needless_pass_by_value)]
    fn py_is_active(slf: PyRef<'_, Self>) -> bool {
        slf.is_active()
    }

    /// Check if the client is reconnecting.
    ///
    /// Returns `true` if the client lost connection and is attempting to reestablish it.
    /// The client will automatically retry connection based on its configuration.
    #[pyo3(name = "is_reconnecting")]
    #[expect(clippy::needless_pass_by_value)]
    fn py_is_reconnecting(slf: PyRef<'_, Self>) -> bool {
        slf.is_reconnecting()
    }

    /// Check if the client is disconnecting.
    ///
    /// Returns `true` if the client is in disconnect mode.
    #[pyo3(name = "is_disconnecting")]
    #[expect(clippy::needless_pass_by_value)]
    fn py_is_disconnecting(slf: PyRef<'_, Self>) -> bool {
        slf.is_disconnecting()
    }

    /// Check if the client is closed.
    ///
    /// Returns `true` if the client has been explicitly disconnected or reached
    /// maximum reconnection attempts. In this state, the client cannot be reused
    /// and a new client must be created for further connections.
    #[pyo3(name = "is_closed")]
    #[expect(clippy::needless_pass_by_value)]
    fn py_is_closed(slf: PyRef<'_, Self>) -> bool {
        slf.is_closed()
    }

    #[pyo3(name = "mode")]
    #[expect(clippy::needless_pass_by_value)]
    fn py_mode(slf: PyRef<'_, Self>) -> String {
        slf.connection_mode().to_string()
    }

    /// Reconnect the client.
    #[pyo3(name = "reconnect")]
    #[expect(clippy::needless_pass_by_value)]
    fn py_reconnect<'py>(slf: PyRef<'_, Self>, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let connection_mode = slf.connection_mode.clone();
        let state_notify = slf.state_notify.clone();
        let mode_str = ConnectionMode::from_atomic(&connection_mode).to_string();
        log::debug!("Reconnect from mode {mode_str}");

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            match ConnectionMode::from_atomic(&connection_mode) {
                ConnectionMode::Reconnect => {
                    log::warn!("Cannot reconnect - socket already reconnecting");
                }
                ConnectionMode::Disconnect => {
                    log::warn!("Cannot reconnect - socket disconnecting");
                }
                ConnectionMode::Closed => {
                    log::warn!("Cannot reconnect - socket closed");
                }
                ConnectionMode::Active => {
                    connection_mode.store(ConnectionMode::Reconnect.as_u8(), Ordering::SeqCst);
                    state_notify.notify_one();

                    let fallback_interval = Duration::from_millis(100);
                    let timeout = tokio::time::timeout(Duration::from_secs(30), async {
                        loop {
                            let notified = state_notify.notified();

                            let current = ConnectionMode::from_atomic(&connection_mode);
                            if current.is_active() {
                                return Ok(());
                            }

                            if current.is_closed() || current.is_disconnect() {
                                return Err("Connection closed during reconnect");
                            }

                            tokio::select! {
                                biased;
                                () = notified => {}
                                () = tokio::time::sleep(fallback_interval) => {}
                            }
                        }
                    })
                    .await;

                    match timeout {
                        Ok(Ok(())) => log::debug!("Reconnected successfully"),
                        Ok(Err(e)) => log::warn!("Reconnect aborted: {e}"),
                        Err(_) => log::error!("Reconnect timed out after 30s"),
                    }
                }
            }

            Ok(())
        })
    }

    /// Close the client.
    ///
    /// Controller task will periodically check the disconnect mode
    /// and shutdown the client if it is not alive.
    #[pyo3(name = "close")]
    #[expect(clippy::needless_pass_by_value)]
    fn py_close<'py>(slf: PyRef<'_, Self>, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let connection_mode = slf.connection_mode.clone();
        let state_notify = slf.state_notify.clone();
        let mode_str = ConnectionMode::from_atomic(&connection_mode).to_string();
        log::debug!("Close from mode {mode_str}");

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            match ConnectionMode::from_atomic(&connection_mode) {
                ConnectionMode::Closed => {
                    log::debug!("Socket already closed");
                }
                ConnectionMode::Disconnect => {
                    log::debug!("Socket already disconnecting");
                }
                _ => {
                    connection_mode.store(ConnectionMode::Disconnect.as_u8(), Ordering::SeqCst);
                    state_notify.notify_one();

                    let timeout = tokio::time::timeout(Duration::from_secs(5), async {
                        while !ConnectionMode::from_atomic(&connection_mode).is_closed() {
                            tokio::time::sleep(Duration::from_millis(10)).await;
                        }
                    })
                    .await;

                    if timeout.is_err() {
                        log::error!("Timeout waiting for socket to close, forcing closed state");
                        connection_mode.store(ConnectionMode::Closed.as_u8(), Ordering::SeqCst);
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
    #[expect(clippy::needless_pass_by_value)]
    fn py_send<'py>(
        slf: PyRef<'_, Self>,
        data: Vec<u8>,
        py: Python<'py>,
    ) -> PyResult<Bound<'py, PyAny>> {
        log::trace!("Sending {}", String::from_utf8_lossy(&data));

        let connection_mode = slf.connection_mode.clone();
        let state_notify = slf.state_notify.clone();
        let writer_tx = slf.writer_tx.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            match ConnectionMode::from_atomic(&connection_mode) {
                ConnectionMode::Disconnect | ConnectionMode::Closed => {
                    let msg = format!(
                        "Cannot send data ({}): socket closed",
                        String::from_utf8_lossy(&data)
                    );

                    let io_err = std::io::Error::new(std::io::ErrorKind::NotConnected, msg);
                    return Err(to_pyruntime_err(io_err));
                }
                mode if !mode.is_active() => {
                    let timeout = Duration::from_secs(2);
                    let fallback_interval = Duration::from_millis(100);

                    log::debug!("Waiting for client to become ACTIVE before sending (2s)...");

                    match tokio::time::timeout(timeout, async {
                        loop {
                            let notified = state_notify.notified();

                            let mode = ConnectionMode::from_atomic(&connection_mode);
                            if mode.is_active() {
                                return Ok(());
                            }

                            if matches!(mode, ConnectionMode::Disconnect | ConnectionMode::Closed) {
                                return Err("Client disconnected waiting to send");
                            }

                            tokio::select! {
                                biased;
                                () = notified => {}
                                () = tokio::time::sleep(fallback_interval) => {}
                            }
                        }
                    })
                    .await
                    {
                        Ok(Ok(())) => log::debug!("Client now active"),
                        Ok(Err(e)) => {
                            let err_msg = format!(
                                "Failed sending data ({}): {e}",
                                String::from_utf8_lossy(&data)
                            );

                            let io_err =
                                std::io::Error::new(std::io::ErrorKind::NotConnected, err_msg);
                            return Err(to_pyruntime_err(io_err));
                        }
                        Err(_) => {
                            let err_msg = format!(
                                "Failed sending data ({}): timeout waiting to become ACTIVE",
                                String::from_utf8_lossy(&data)
                            );

                            let io_err = std::io::Error::new(std::io::ErrorKind::TimedOut, err_msg);
                            return Err(to_pyruntime_err(io_err));
                        }
                    }
                }
                _ => {}
            }

            let msg = WriterCommand::Send(data.into());
            writer_tx.send(msg).map_err(to_pyruntime_err)
        })
    }
}

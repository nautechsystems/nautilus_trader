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

use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

use nautilus_core::{
    collections::into_ustr_vec,
    python::{clone_py_object, to_pyruntime_err, to_pyvalue_err},
};
use pyo3::{Py, create_exception, exceptions::PyException, prelude::*, types::PyBytes};
use tokio_tungstenite::tungstenite::{Message, Utf8Bytes};

use crate::{
    RECONNECTED,
    mode::ConnectionMode,
    ratelimiter::quota::Quota,
    websocket::{
        WebSocketClient, WebSocketConfig,
        types::{MessageHandler, PingHandler, WriterCommand},
    },
};

create_exception!(network, WebSocketClientError, PyException);

fn to_websocket_pyerr(e: tokio_tungstenite::tungstenite::Error) -> PyErr {
    PyErr::new::<WebSocketClientError, _>(e.to_string())
}

#[pymethods]
impl WebSocketConfig {
    /// Create a new WebSocket configuration.
    #[new]
    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (
        url,
        headers,
        heartbeat=None,
        heartbeat_msg=None,
        reconnect_timeout_ms=10_000,
        reconnect_delay_initial_ms=2_000,
        reconnect_delay_max_ms=30_000,
        reconnect_backoff_factor=1.5,
        reconnect_jitter_ms=100,
        reconnect_max_attempts=None,
    ))]
    fn py_new(
        url: String,
        headers: Vec<(String, String)>,
        heartbeat: Option<u64>,
        heartbeat_msg: Option<String>,
        reconnect_timeout_ms: Option<u64>,
        reconnect_delay_initial_ms: Option<u64>,
        reconnect_delay_max_ms: Option<u64>,
        reconnect_backoff_factor: Option<f64>,
        reconnect_jitter_ms: Option<u64>,
        reconnect_max_attempts: Option<u32>,
    ) -> Self {
        Self {
            url,
            headers,
            heartbeat,
            heartbeat_msg,
            reconnect_timeout_ms,
            reconnect_delay_initial_ms,
            reconnect_delay_max_ms,
            reconnect_backoff_factor,
            reconnect_jitter_ms,
            reconnect_max_attempts,
        }
    }
}

#[pymethods]
impl WebSocketClient {
    /// Create a websocket client.
    ///
    /// The handler and ping_handler callbacks are scheduled on the provided event loop
    /// using `call_soon_threadsafe` to ensure they execute on the correct thread.
    /// This is critical for thread safety since WebSocket messages arrive on
    /// a Tokio worker thread, but Python callbacks (like those entering the
    /// kernel via MessageBus) must run on the asyncio event loop thread.
    ///
    /// # Safety
    ///
    /// - Throws an Exception if it is unable to make websocket connection.
    #[staticmethod]
    #[pyo3(name = "connect", signature = (loop_, config, handler, ping_handler = None, post_reconnection = None, keyed_quotas = Vec::new(), default_quota = None))]
    #[allow(clippy::too_many_arguments)]
    fn py_connect(
        loop_: Py<PyAny>,
        config: WebSocketConfig,
        handler: Py<PyAny>,
        ping_handler: Option<Py<PyAny>>,
        post_reconnection: Option<Py<PyAny>>,
        keyed_quotas: Vec<(String, Quota)>,
        default_quota: Option<Quota>,
        py: Python<'_>,
    ) -> PyResult<Bound<'_, PyAny>> {
        let call_soon_threadsafe: Py<PyAny> = loop_.getattr(py, "call_soon_threadsafe")?;
        let call_soon_clone = clone_py_object(&call_soon_threadsafe);
        let handler_clone = clone_py_object(&handler);

        let message_handler: MessageHandler = Arc::new(move |msg: Message| {
            if matches!(msg, Message::Text(ref text) if text.as_str() == RECONNECTED) {
                return;
            }

            Python::attach(|py| {
                let py_bytes = match &msg {
                    Message::Binary(data) => PyBytes::new(py, data),
                    Message::Text(text) => PyBytes::new(py, text.as_bytes()),
                    _ => return,
                };

                if let Err(e) = call_soon_clone.call1(py, (&handler_clone, py_bytes)) {
                    log::error!("Error scheduling message handler on event loop: {e}");
                }
            });
        });

        let ping_handler_fn = ping_handler.map(|ping_handler| {
            let ping_handler_clone = clone_py_object(&ping_handler);
            let call_soon_clone = clone_py_object(&call_soon_threadsafe);

            let ping_handler_fn: PingHandler = Arc::new(move |data: Vec<u8>| {
                Python::attach(|py| {
                    let py_bytes = PyBytes::new(py, &data);
                    if let Err(e) = call_soon_clone.call1(py, (&ping_handler_clone, py_bytes)) {
                        log::error!("Error scheduling ping handler on event loop: {e}");
                    }
                });
            });
            ping_handler_fn
        });

        let post_reconnection_fn = post_reconnection.map(|callback| {
            let callback_clone = clone_py_object(&callback);
            Arc::new(move || {
                Python::attach(|py| {
                    if let Err(e) = callback_clone.call0(py) {
                        log::error!("Error calling post_reconnection handler: {e}");
                    }
                });
            }) as std::sync::Arc<dyn Fn() + Send + Sync>
        });

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            Self::connect(
                config,
                Some(message_handler),
                ping_handler_fn,
                post_reconnection_fn,
                keyed_quotas,
                default_quota,
            )
            .await
            .map_err(to_websocket_pyerr)
        })
    }

    /// Closes the client heart beat and reader task.
    ///
    /// The connection is not completely closed the till all references
    /// to the client are gone and the client is dropped.
    ///
    /// # Safety
    ///
    /// - The client should not be used after closing it.
    /// - Any auto-reconnect job should be aborted before closing the client.
    #[pyo3(name = "disconnect")]
    fn py_disconnect<'py>(slf: PyRef<'_, Self>, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let connection_mode = slf.connection_mode.clone();
        let mode = ConnectionMode::from_atomic(&connection_mode);
        log::debug!("Close from mode {mode}");

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            match ConnectionMode::from_atomic(&connection_mode) {
                ConnectionMode::Closed => {
                    log::debug!("WebSocket already closed");
                }
                ConnectionMode::Disconnect => {
                    log::debug!("WebSocket already disconnecting");
                }
                _ => {
                    connection_mode.store(ConnectionMode::Disconnect.as_u8(), Ordering::SeqCst);
                    while !ConnectionMode::from_atomic(&connection_mode).is_closed() {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                }
            }

            Ok(())
        })
    }

    /// Check if the client is still alive.
    ///
    /// Even if the connection is disconnected the client will still be alive
    /// and trying to reconnect.
    ///
    /// This is particularly useful for checking why a `send` failed. It could
    /// be because the connection disconnected and the client is still alive
    /// and reconnecting. In such cases the send can be retried after some
    /// delay.
    #[pyo3(name = "is_active")]
    fn py_is_active(slf: PyRef<'_, Self>) -> bool {
        !slf.controller_task.is_finished()
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

    /// Send bytes data to the server.
    ///
    /// # Errors
    ///
    /// - Raises `PyRuntimeError` if not able to send data.
    #[pyo3(name = "send")]
    #[pyo3(signature = (data, keys=None))]
    fn py_send<'py>(
        slf: PyRef<'_, Self>,
        data: Vec<u8>,
        py: Python<'py>,
        keys: Option<Vec<String>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let rate_limiter = slf.rate_limiter.clone();
        let writer_tx = slf.writer_tx.clone();
        let mode = slf.connection_mode.clone();
        let keys = keys.map(into_ustr_vec);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if !ConnectionMode::from_atomic(&mode).is_active() {
                let msg = "Cannot send data: connection not active".to_string();
                log::error!("{msg}");
                return Err(to_pyruntime_err(std::io::Error::new(
                    std::io::ErrorKind::NotConnected,
                    msg,
                )));
            }
            rate_limiter.await_keys_ready(keys.as_deref()).await;
            log::trace!("Sending binary: {data:?}");

            let msg = Message::Binary(data.into());
            writer_tx
                .send(WriterCommand::Send(msg))
                .map_err(to_pyruntime_err)
        })
    }

    /// Send UTF-8 encoded bytes as text data to the server, respecting rate limits.
    ///
    /// `data`: The byte data to be sent, which will be converted to a UTF-8 string.
    /// `keys`: Optional list of rate limit keys. If provided, the function will wait for rate limits to be met for each key before sending the data.
    ///
    /// # Errors
    /// - Raises `PyRuntimeError` if unable to send the data.
    ///
    /// # Example
    ///
    /// When a request is made the URL should be split into all relevant keys within it.
    ///
    /// For request /foo/bar, should pass keys ["foo/bar", "foo"] for rate limiting.
    #[pyo3(name = "send_text")]
    #[pyo3(signature = (data, keys=None))]
    fn py_send_text<'py>(
        slf: PyRef<'_, Self>,
        data: Vec<u8>,
        py: Python<'py>,
        keys: Option<Vec<String>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let data_str = String::from_utf8(data).map_err(to_pyvalue_err)?;
        let data = Utf8Bytes::from(data_str);
        let rate_limiter = slf.rate_limiter.clone();
        let writer_tx = slf.writer_tx.clone();
        let mode = slf.connection_mode.clone();
        let keys = keys.map(into_ustr_vec);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if !ConnectionMode::from_atomic(&mode).is_active() {
                let e = std::io::Error::new(
                    std::io::ErrorKind::NotConnected,
                    "Cannot send text: connection not active",
                );
                return Err(to_pyruntime_err(e));
            }
            rate_limiter.await_keys_ready(keys.as_deref()).await;
            log::trace!("Sending text: {data}");

            let msg = Message::Text(data);
            writer_tx
                .send(WriterCommand::Send(msg))
                .map_err(to_pyruntime_err)
        })
    }

    /// Send pong bytes data to the server.
    ///
    /// # Errors
    ///
    /// - Raises `PyRuntimeError` if not able to send data.
    #[pyo3(name = "send_pong")]
    fn py_send_pong<'py>(
        slf: PyRef<'_, Self>,
        data: Vec<u8>,
        py: Python<'py>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let writer_tx = slf.writer_tx.clone();
        let mode = slf.connection_mode.clone();
        let data_len = data.len();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if !ConnectionMode::from_atomic(&mode).is_active() {
                let e = std::io::Error::new(
                    std::io::ErrorKind::NotConnected,
                    "Cannot send pong: connection not active",
                );
                return Err(to_pyruntime_err(e));
            }
            log::trace!("Sending pong frame ({data_len} bytes)");

            let msg = Message::Pong(data.into());
            writer_tx
                .send(WriterCommand::Send(msg))
                .map_err(to_pyruntime_err)
        })
    }
}

#[cfg(test)]
#[cfg(target_os = "linux")] // Only run network tests on Linux (CI stability)
mod tests {
    use std::ffi::CString;

    use futures_util::{SinkExt, StreamExt};
    use nautilus_core::python::IntoPyObjectNautilusExt;
    use pyo3::{prelude::*, types::PyBytes};
    use tokio::{
        net::TcpListener,
        task::{self, JoinHandle},
        time::{Duration, sleep},
    };
    use tokio_tungstenite::{
        accept_hdr_async,
        tungstenite::{
            Message,
            handshake::server::{self, Callback},
            http::HeaderValue,
        },
    };

    use crate::websocket::{MessageHandler, WebSocketClient, WebSocketConfig};

    struct TestServer {
        task: JoinHandle<()>,
        port: u16,
    }

    #[derive(Debug, Clone)]
    struct TestCallback {
        key: String,
        value: HeaderValue,
    }

    impl Callback for TestCallback {
        #[allow(clippy::panic_in_result_fn)]
        fn on_request(
            self,
            request: &server::Request,
            response: server::Response,
        ) -> Result<server::Response, server::ErrorResponse> {
            let _ = response;
            let value = request.headers().get(&self.key);
            assert!(value.is_some());

            if let Some(value) = request.headers().get(&self.key) {
                assert_eq!(value, self.value);
            }

            Ok(response)
        }
    }

    impl TestServer {
        async fn setup(key: String, value: String) -> Self {
            let server = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = TcpListener::local_addr(&server).unwrap().port();

            let test_call_back = TestCallback {
                key,
                value: HeaderValue::from_str(&value).unwrap(),
            };

            // Set up test server
            let task = task::spawn(async move {
                // Keep accepting connections
                loop {
                    let (conn, _) = server.accept().await.unwrap();
                    let mut websocket = accept_hdr_async(conn, test_call_back.clone())
                        .await
                        .unwrap();

                    task::spawn(async move {
                        while let Some(Ok(msg)) = websocket.next().await {
                            match msg {
                                tokio_tungstenite::tungstenite::protocol::Message::Text(txt)
                                    if txt == "close-now" =>
                                {
                                    log::debug!("Forcibly closing from server side");
                                    // This sends a close frame, then stops reading
                                    let _ = websocket.close(None).await;
                                    break;
                                }
                                // Echo text/binary frames
                                tokio_tungstenite::tungstenite::protocol::Message::Text(_)
                                | tokio_tungstenite::tungstenite::protocol::Message::Binary(_) => {
                                    if websocket.send(msg).await.is_err() {
                                        break;
                                    }
                                }
                                // If the client closes, we also break
                                tokio_tungstenite::tungstenite::protocol::Message::Close(
                                    _frame,
                                ) => {
                                    let _ = websocket.close(None).await;
                                    break;
                                }
                                // Ignore pings/pongs
                                _ => {}
                            }
                        }
                    });
                }
            });

            Self { task, port }
        }
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    fn create_test_handler() -> (Py<PyAny>, Py<PyAny>) {
        let code_raw = r"
class Counter:
    def __init__(self):
        self.count = 0
        self.check = False

    def handler(self, bytes):
        msg = bytes.decode()
        if msg == 'ping':
            self.count += 1
        elif msg == 'heartbeat message':
            self.check = True

    def get_check(self):
        return self.check

    def get_count(self):
        return self.count

counter = Counter()
";

        let code = CString::new(code_raw).unwrap();
        let filename = CString::new("test".to_string()).unwrap();
        let module = CString::new("test".to_string()).unwrap();
        Python::attach(|py| {
            let pymod = PyModule::from_code(py, &code, &filename, &module).unwrap();

            let counter = pymod.getattr("counter").unwrap().into_py_any_unwrap(py);
            let handler = counter
                .getattr(py, "handler")
                .unwrap()
                .into_py_any_unwrap(py);

            (counter, handler)
        })
    }

    #[tokio::test]
    async fn basic_client_test() {
        const N: usize = 10;

        Python::initialize();

        let mut success_count = 0;
        let header_key = "hello-custom-key".to_string();
        let header_value = "hello-custom-value".to_string();

        let server = TestServer::setup(header_key.clone(), header_value.clone()).await;
        let (counter, handler) = create_test_handler();

        let config = WebSocketConfig::py_new(
            format!("ws://127.0.0.1:{}", server.port),
            vec![(header_key, header_value)],
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );

        let handler_clone = Python::attach(|py| handler.clone_ref(py));

        let message_handler: MessageHandler = std::sync::Arc::new(move |msg: Message| {
            Python::attach(|py| {
                let data = match msg {
                    Message::Binary(data) => data.to_vec(),
                    Message::Text(text) => text.as_bytes().to_vec(),
                    _ => return,
                };
                let py_bytes = PyBytes::new(py, &data);
                if let Err(e) = handler_clone.call1(py, (py_bytes,)) {
                    log::error!("Error calling handler: {e}");
                }
            });
        });

        let client =
            WebSocketClient::connect(config, Some(message_handler), None, None, vec![], None)
                .await
                .unwrap();

        for _ in 0..N {
            client.send_bytes(b"ping".to_vec(), None).await.unwrap();
            success_count += 1;
        }

        sleep(Duration::from_secs(1)).await;
        let count_value: usize = Python::attach(|py| {
            counter
                .getattr(py, "get_count")
                .unwrap()
                .call0(py)
                .unwrap()
                .extract(py)
                .unwrap()
        });
        assert_eq!(count_value, success_count);

        // Close the connection => client should reconnect automatically
        client.send_close_message().await.unwrap();

        // Send messages that increment the count
        sleep(Duration::from_secs(2)).await;
        for _ in 0..N {
            client.send_bytes(b"ping".to_vec(), None).await.unwrap();
            success_count += 1;
        }

        sleep(Duration::from_secs(1)).await;
        let count_value: usize = Python::attach(|py| {
            counter
                .getattr(py, "get_count")
                .unwrap()
                .call0(py)
                .unwrap()
                .extract(py)
                .unwrap()
        });
        assert_eq!(count_value, success_count);
        assert_eq!(success_count, N + N);

        client.disconnect().await;
        assert!(client.is_disconnected());
    }

    #[tokio::test]
    async fn message_ping_test() {
        Python::initialize();

        let header_key = "hello-custom-key".to_string();
        let header_value = "hello-custom-value".to_string();

        let (checker, handler) = create_test_handler();

        let server = TestServer::setup(header_key.clone(), header_value.clone()).await;
        let config = WebSocketConfig::py_new(
            format!("ws://127.0.0.1:{}", server.port),
            vec![(header_key, header_value)],
            Some(1),
            Some("heartbeat message".to_string()),
            None,
            None,
            None,
            None,
            None,
            None,
        );

        let handler_clone = Python::attach(|py| handler.clone_ref(py));

        let message_handler: MessageHandler = std::sync::Arc::new(move |msg: Message| {
            Python::attach(|py| {
                let data = match msg {
                    Message::Binary(data) => data.to_vec(),
                    Message::Text(text) => text.as_bytes().to_vec(),
                    _ => return,
                };
                let py_bytes = PyBytes::new(py, &data);
                if let Err(e) = handler_clone.call1(py, (py_bytes,)) {
                    log::error!("Error calling handler: {e}");
                }
            });
        });

        let client =
            WebSocketClient::connect(config, Some(message_handler), None, None, vec![], None)
                .await
                .unwrap();

        sleep(Duration::from_secs(2)).await;
        let check_value: bool = Python::attach(|py| {
            checker
                .getattr(py, "get_check")
                .unwrap()
                .call0(py)
                .unwrap()
                .extract(py)
                .unwrap()
        });
        assert!(check_value);

        client.disconnect().await;
        assert!(client.is_disconnected());
    }
}

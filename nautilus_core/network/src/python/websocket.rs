// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use futures::SinkExt;
use futures_util::{stream, StreamExt};
use nautilus_core::python::to_pyvalue_err;
use pyo3::{create_exception, exceptions::PyException, prelude::*};
use tokio_tungstenite::tungstenite::Message;

use crate::{
    ratelimiter::quota::Quota,
    websocket::{WebSocketClient, WebSocketConfig},
};

// Python exception class for websocket errors
create_exception!(network, WebSocketClientError, PyException);

fn to_websocket_pyerr(e: tokio_tungstenite::tungstenite::Error) -> PyErr {
    PyErr::new::<WebSocketClientError, _>(e.to_string())
}

#[pymethods]
impl WebSocketConfig {
    #[new]
    #[pyo3(signature = (url, handler, headers, heartbeat=None, heartbeat_msg=None, ping_handler=None))]
    fn py_new(
        url: String,
        handler: PyObject,
        headers: Vec<(String, String)>,
        heartbeat: Option<u64>,
        heartbeat_msg: Option<String>,
        ping_handler: Option<PyObject>,
    ) -> Self {
        Self {
            url,
            handler: Arc::new(handler),
            headers,
            heartbeat,
            heartbeat_msg,
            ping_handler: ping_handler.map(Arc::new),
        }
    }
}

#[pymethods]
impl WebSocketClient {
    /// Create a websocket client.
    ///
    /// # Safety
    ///
    /// - Throws an Exception if it is unable to make websocket connection.
    #[staticmethod]
    #[pyo3(name = "connect", signature = (config, post_connection= None, post_reconnection= None, post_disconnection= None, keyed_quotas = Vec::new(),default_quota = None))]
    fn py_connect(
        config: WebSocketConfig,
        post_connection: Option<PyObject>,
        post_reconnection: Option<PyObject>,
        post_disconnection: Option<PyObject>,
        keyed_quotas: Vec<(String, Quota)>,
        default_quota: Option<Quota>,
        py: Python<'_>,
    ) -> PyResult<Bound<PyAny>> {
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            Self::connect(
                config,
                post_connection,
                post_reconnection,
                post_disconnection,
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
        let disconnect_mode = slf.disconnect_mode.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            disconnect_mode.store(true, Ordering::SeqCst);
            Ok(())
        })
    }

    /// Check if the client is still alive.
    ///
    /// Even if the connection is disconnected the client will still be alive
    /// and trying to reconnect. Only when reconnect fails the client will
    /// terminate.
    ///
    /// This is particularly useful for checking why a `send` failed. It could
    /// be because the connection disconnected and the client is still alive
    /// and reconnecting. In such cases the send can be retried after some
    /// delay.
    #[pyo3(name = "is_alive")]
    fn py_is_alive(slf: PyRef<'_, Self>) -> bool {
        !slf.controller_task.is_finished()
    }

    /// Send bytes data to the server.
    ///
    /// # Errors
    ///
    /// - Raises PyRuntimeError if not able to send data.
    #[pyo3(name = "send")]
    #[pyo3(signature = (data, keys=None))]
    fn py_send<'py>(
        slf: PyRef<'_, Self>,
        data: Vec<u8>,
        py: Python<'py>,
        keys: Option<Vec<String>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let keys = keys.unwrap_or_default();
        let writer = slf.writer.clone();
        let rate_limiter = slf.rate_limiter.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let tasks = keys.iter().map(|key| rate_limiter.until_key_ready(key));
            stream::iter(tasks)
                .for_each(|key| async move {
                    key.await;
                })
                .await;

            // Log after passing rate limit checks
            tracing::trace!("Sending binary: {data:?}");

            let mut guard = writer.lock().await;
            guard
                .send(Message::Binary(data))
                .await
                .map_err(to_websocket_pyerr)
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
        let data = String::from_utf8(data).map_err(to_pyvalue_err)?;
        let keys = keys.unwrap_or_default();
        let writer = slf.writer.clone();
        let rate_limiter = slf.rate_limiter.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let tasks = keys.iter().map(|key| rate_limiter.until_key_ready(key));
            stream::iter(tasks)
                .for_each(|key| async move {
                    key.await;
                })
                .await;

            // Log after passing rate limit checks
            tracing::trace!("Sending text: {data}");

            let mut guard = writer.lock().await;
            guard
                .send(Message::Text(data))
                .await
                .map_err(to_websocket_pyerr)
        })
    }

    /// Send pong bytes data to the server.
    ///
    /// # Errors
    ///
    /// - Raises PyRuntimeError if not able to send data.
    #[pyo3(name = "send_pong")]
    fn py_send_pong<'py>(
        slf: PyRef<'_, Self>,
        data: Vec<u8>,
        py: Python<'py>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let data_str = String::from_utf8(data.clone()).map_err(to_pyvalue_err)?;
        tracing::trace!("Sending pong: {data_str}");
        let writer = slf.writer.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut guard = writer.lock().await;
            guard
                .send(Message::Pong(data))
                .await
                .map_err(to_websocket_pyerr)
        })
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use futures_util::{SinkExt, StreamExt};
    use pyo3::{prelude::*, prepare_freethreaded_python};
    use tokio::{
        net::TcpListener,
        task::{self, JoinHandle},
        time::{sleep, Duration},
    };
    use tokio_tungstenite::{
        accept_hdr_async,
        tungstenite::{
            handshake::server::{self, Callback},
            http::HeaderValue,
        },
    };
    use tracing_test::traced_test;

    use crate::websocket::{WebSocketClient, WebSocketConfig};

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
                        loop {
                            let msg = websocket.next().await.unwrap().unwrap();
                            // We do not want to send back ping/pong messages.
                            if msg.is_binary() || msg.is_text() {
                                websocket.send(msg).await.unwrap();
                            } else if msg.is_close() {
                                if let Err(e) = websocket.close(None).await {
                                    tracing::debug!("Connection already closed {e}");
                                };
                                break;
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

    #[tokio::test]
    #[traced_test]
    async fn basic_client_test() {
        prepare_freethreaded_python();

        const N: usize = 10;
        let mut success_count = 0;
        let header_key = "hello-custom-key".to_string();
        let header_value = "hello-custom-value".to_string();

        // Initialize test server
        let server = TestServer::setup(header_key.clone(), header_value.clone()).await;

        // Create counter class and handler that increments it
        let (counter, handler) = Python::with_gil(|py| {
            let pymod = PyModule::from_code_bound(
                py,
                r"
class Counter:
    def __init__(self):
        self.count = 0

    def handler(self, bytes):
        if bytes.decode() == 'ping':
            self.count = self.count + 1

    def get_count(self):
        return self.count

counter = Counter()",
                "",
                "",
            )
            .unwrap();

            let counter = pymod.getattr("counter").unwrap().into_py(py);
            let handler = counter.getattr(py, "handler").unwrap().into_py(py);

            (counter, handler)
        });

        let config = WebSocketConfig::py_new(
            format!("ws://127.0.0.1:{}", server.port),
            Python::with_gil(|py| handler.clone_ref(py)),
            vec![(header_key, header_value)],
            None,
            None,
            None,
        );
        let client = WebSocketClient::connect(config, None, None, None, Vec::new(), None)
            .await
            .unwrap();

        // Send messages that increment the count
        for _ in 0..N {
            if client.send_bytes(b"ping".to_vec()).await.is_ok() {
                success_count += 1;
            };
        }

        // Check count is same as number messages sent
        sleep(Duration::from_secs(1)).await;
        let count_value: usize = Python::with_gil(|py| {
            counter
                .getattr(py, "get_count")
                .unwrap()
                .call0(py)
                .unwrap()
                .extract(py)
                .unwrap()
        });
        assert_eq!(count_value, success_count);

        //////////////////////////////////////////////////////////////////////
        // Close connection client should reconnect and send messages
        //////////////////////////////////////////////////////////////////////

        // close the connection
        // client should reconnect automatically
        client.send_close_message().await;

        // Send messages that increment the count
        sleep(Duration::from_secs(2)).await;
        for _ in 0..N {
            if client.send_bytes(b"ping".to_vec()).await.is_ok() {
                success_count += 1;
            };
        }

        // Check count is same as number messages sent
        sleep(Duration::from_secs(1)).await;
        let count_value: usize = Python::with_gil(|py| {
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

        // Shutdown client
        client.disconnect().await;
        assert!(client.is_disconnected());
    }

    #[tokio::test]
    #[traced_test]
    async fn message_ping_test() {
        prepare_freethreaded_python();

        let header_key = "hello-custom-key".to_string();
        let header_value = "hello-custom-value".to_string();

        let (checker, handler) = Python::with_gil(|py| {
            let pymod = PyModule::from_code_bound(
                py,
                r"
class Checker:
    def __init__(self):
        self.check = False

    def handler(self, bytes):
        if bytes.decode() == 'heartbeat message':
            self.check = True

    def get_check(self):
        return self.check

checker = Checker()",
                "",
                "",
            )
            .unwrap();

            let checker = pymod.getattr("checker").unwrap().into_py(py);
            let handler = checker.getattr(py, "handler").unwrap().into_py(py);

            (checker, handler)
        });

        // Initialize test server and config
        let server = TestServer::setup(header_key.clone(), header_value.clone()).await;
        let config = WebSocketConfig::py_new(
            format!("ws://127.0.0.1:{}", server.port),
            Python::with_gil(|py| handler.clone_ref(py)),
            vec![(header_key, header_value)],
            Some(1),
            Some("heartbeat message".to_string()),
            None,
        );
        let client = WebSocketClient::connect(config, None, None, None, Vec::new(), None)
            .await
            .unwrap();

        // Check if ping message has the correct message
        sleep(Duration::from_secs(2)).await;
        let check_value: bool = Python::with_gil(|py| {
            checker
                .getattr(py, "get_check")
                .unwrap()
                .call0(py)
                .unwrap()
                .extract(py)
                .unwrap()
        });
        assert!(check_value);

        // Shutdown client
        client.disconnect().await;
        assert!(client.is_disconnected());
    }
}

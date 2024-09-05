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

use std::{
    collections::{hash_map::DefaultHasher, HashMap},
    hash::{Hash, Hasher},
    sync::{atomic::Ordering, Arc},
};

use futures_util::{stream, StreamExt};
use nautilus_core::python::to_pyruntime_err;
use pyo3::{exceptions::PyException, prelude::*, types::PyBytes};
use tokio::io::AsyncWriteExt;
use tokio_tungstenite::tungstenite::stream::Mode;

use crate::{
    http::{HttpClient, HttpMethod, HttpResponse, InnerHttpClient},
    ratelimiter::{quota::Quota, RateLimiter},
    socket::{SocketClient, SocketConfig},
};

#[pymethods]
impl SocketConfig {
    #[new]
    fn py_new(
        url: String,
        ssl: bool,
        suffix: Vec<u8>,
        handler: PyObject,
        heartbeat: Option<(u64, Vec<u8>)>,
    ) -> Self {
        let mode = if ssl { Mode::Tls } else { Mode::Plain };
        Self {
            url,
            mode,
            suffix,
            handler,
            heartbeat,
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
    fn py_connect(
        config: SocketConfig,
        post_connection: Option<PyObject>,
        post_reconnection: Option<PyObject>,
        post_disconnection: Option<PyObject>,
        py: Python<'_>,
    ) -> PyResult<Bound<PyAny>> {
        pyo3_asyncio_0_21::tokio::future_into_py(py, async move {
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
        pyo3_asyncio_0_21::tokio::future_into_py(py, async move {
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

        pyo3_asyncio_0_21::tokio::future_into_py(py, async move {
            let mut writer = writer.lock().await;
            writer.write_all(&data).await?;
            Ok(())
        })
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use pyo3::{prelude::*, prepare_freethreaded_python};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        task::{self, JoinHandle},
        time::{sleep, Duration},
    };
    use tokio_tungstenite::tungstenite::stream::Mode;
    use tracing_test::traced_test;

    use crate::socket::{SocketClient, SocketConfig};

    struct TestServer {
        task: JoinHandle<()>,
        port: u16,
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    impl TestServer {
        async fn basic_client_test() -> Self {
            let server = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = TcpListener::local_addr(&server).unwrap().port();

            // Set up test server
            let handle = task::spawn(async move {
                // Keep listening for new connections
                loop {
                    let (mut stream, _) = server.accept().await.unwrap();
                    tracing::debug!("socket:test Server accepted connection");

                    // Keep receiving messages from connection and sending them back as it is
                    // if the message contains a close stop receiving messages
                    // and drop the connection.
                    task::spawn(async move {
                        let mut buf = Vec::new();
                        loop {
                            let bytes = stream.read_buf(&mut buf).await.unwrap();
                            tracing::debug!("socket:test Server received {bytes} bytes");

                            // Terminate if 0 bytes have been read
                            // Connection has been terminated or vector buffer is completely
                            if bytes == 0 {
                                break;
                            } else {
                                // if received data has a line break
                                // extract and write it to the stream
                                while let Some((i, _)) =
                                    &buf.windows(2).enumerate().find(|(_, pair)| pair == b"\r\n")
                                {
                                    let close_message = b"close".as_slice();
                                    if &buf[0..*i] == close_message {
                                        tracing::debug!("socket:test Client sent closing message");
                                        return;
                                    } else {
                                        tracing::debug!("socket:test Server sending message");
                                        stream
                                            .write_all(buf.drain(0..i + 2).as_slice())
                                            .await
                                            .unwrap();
                                    }
                                }
                            }
                        }
                    });
                }
            });

            Self { task: handle, port }
        }
    }

    #[tokio::test]
    #[traced_test]
    async fn basic_client_test() {
        prepare_freethreaded_python();

        const N: usize = 10;

        // Initialize test server
        let server = TestServer::basic_client_test().await;

        // Create counter class and handler that increments it
        let (counter, handler) = Python::with_gil(|py| {
            let pymod = PyModule::from_code(
                py,
                r"
class Counter:
    def __init__(self):
        self.count = 0

    def handler(self, bytes):
        if bytes.decode().rstrip() == 'ping':
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

        let config = SocketConfig {
            url: format!("127.0.0.1:{}", server.port),
            handler: handler.clone(),
            mode: Mode::Plain,
            suffix: b"\r\n".to_vec(),
            heartbeat: None,
        };
        let client: SocketClient = SocketClient::connect(config, None, None, None)
            .await
            .unwrap();

        // Send messages that increment the count
        for _ in 0..N {
            let _ = client.send_bytes(b"ping".as_slice()).await;
        }

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

        // Check count is same as number messages sent
        assert_eq!(count_value, N);

        //////////////////////////////////////////////////////////////////////
        // Close connection client should reconnect and send messages
        //////////////////////////////////////////////////////////////////////

        // close the connection and wait
        // client should reconnect automatically
        let _ = client.send_bytes(b"close".as_slice()).await;
        sleep(Duration::from_secs(2)).await;

        for _ in 0..N {
            let _ = client.send_bytes(b"ping".as_slice()).await;
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

        // Check that messages were received correctly after reconnecting
        assert_eq!(count_value, N + N);

        // Shutdown client
        client.disconnect().await;
        assert!(client.is_disconnected());
    }
}

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

use std::{sync::Arc, time::Duration};

use nautilus_core::python::to_pyruntime_err;
use pyo3::prelude::*;
use tokio::{
    io::{split, AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf},
    net::TcpStream,
    sync::Mutex,
    task,
    time::sleep,
};
use tokio_tungstenite::{
    tls::tcp_tls,
    tungstenite::{client::IntoClientRequest, stream::Mode, Error},
    MaybeTlsStream,
};
use tracing::{debug, error};

type TcpWriter = WriteHalf<MaybeTlsStream<TcpStream>>;
type SharedTcpWriter = Arc<Mutex<WriteHalf<MaybeTlsStream<TcpStream>>>>;
type TcpReader = ReadHalf<MaybeTlsStream<TcpStream>>;

/// Configuration for TCP socket connection.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.network")
)]
pub struct SocketConfig {
    /// The URL to connect to.
    url: String,
    /// The connection mode {Plain, TLS}.
    mode: Mode,
    /// The sequence of bytes which separates lines.
    suffix: Vec<u8>,
    /// The Python function to handle incoming messages.
    handler: PyObject,
    /// The optional heartbeat with period and beat message.
    heartbeat: Option<(u64, Vec<u8>)>,
}

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

/// Creates a TcpStream with the server
///
/// The stream can be encrypted with TLS or Plain. The stream is split into
/// read and write ends.
/// * The read end is passed to task that keeps receiving
///   messages from the server and passing them to a handler.
/// * The write end is wrapped in an Arc Mutex and used to send messages
///   or heart beats
///
/// The heartbeat is optional and can be configured with an interval and data to
/// send.
///
/// The client uses a suffix to separate messages on the byte stream. It is
/// appended to all sent messages and heartbeats. It is also used the split
/// the received byte stream.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.network")
)]
struct SocketClientInner {
    config: SocketConfig,
    read_task: task::JoinHandle<()>,
    heartbeat_task: Option<task::JoinHandle<()>>,
    writer: SharedTcpWriter,
}

impl SocketClientInner {
    pub async fn connect_url(config: SocketConfig) -> Result<Self, Error> {
        let SocketConfig {
            url,
            mode,
            heartbeat,
            suffix,
            handler,
        } = &config;
        let (reader, writer) = Self::tls_connect_with_server(url, *mode).await?;
        let shared_writer = Arc::new(Mutex::new(writer));

        // Keep receiving messages from socket pass them as arguments to handler
        let read_task = Self::spawn_read_task(reader, handler.clone(), suffix.clone());

        // Optionally create heartbeat task
        let heartbeat_task =
            Self::spawn_heartbeat_task(heartbeat.clone(), shared_writer.clone(), suffix.clone());

        Ok(Self {
            config,
            read_task,
            heartbeat_task,
            writer: shared_writer,
        })
    }

    pub async fn tls_connect_with_server(
        url: &str,
        mode: Mode,
    ) -> Result<(TcpReader, TcpWriter), Error> {
        debug!("Connecting to server");
        let stream = TcpStream::connect(url).await?;
        debug!("Making TLS connection");
        let request = url.into_client_request()?;
        tcp_tls(&request, mode, stream, None).await.map(split)
    }

    #[must_use]
    pub fn spawn_read_task(
        mut reader: TcpReader,
        handler: PyObject,
        suffix: Vec<u8>,
    ) -> task::JoinHandle<()> {
        // Keep receiving messages from socket pass them as arguments to handler
        task::spawn(async move {
            let mut buf = Vec::new();

            loop {
                match reader.read_buf(&mut buf).await {
                    // Connection has been terminated or vector buffer is completely
                    Ok(0) => {
                        error!("Cannot read anymore bytes");
                        break;
                    }
                    Err(e) => {
                        error!("Failed with error: {e}");
                        break;
                    }
                    // Received bytes of data
                    Ok(bytes) => {
                        debug!("Received {bytes} bytes of data");

                        // While received data has a line break
                        // drain it and pass it to the handler
                        while let Some((i, _)) = &buf
                            .windows(suffix.len())
                            .enumerate()
                            .find(|(_, pair)| pair.eq(&suffix))
                        {
                            let mut data: Vec<u8> = buf.drain(0..i + suffix.len()).collect();
                            data.truncate(data.len() - suffix.len());

                            if let Err(e) =
                                Python::with_gil(|py| handler.call1(py, (data.as_slice(),)))
                            {
                                error!("Call to handler failed: {e}");
                                break;
                            }
                        }
                    }
                };
            }
        })
    }

    /// Optionally spawn a heartbeat task to periodically ping the server.
    pub fn spawn_heartbeat_task(
        heartbeat: Option<(u64, Vec<u8>)>,
        writer: SharedTcpWriter,
        suffix: Vec<u8>,
    ) -> Option<task::JoinHandle<()>> {
        heartbeat.map(|(duration, mut message)| {
            task::spawn(async move {
                let duration = Duration::from_secs(duration);
                message.extend(suffix);
                loop {
                    sleep(duration).await;
                    debug!("Sending heartbeat");
                    let mut guard = writer.lock().await;
                    match guard.write_all(&message).await {
                        Ok(()) => debug!("Sent heartbeat"),
                        Err(e) => error!("Failed to send heartbeat: {e}"),
                    }
                }
            })
        })
    }

    /// Shutdown read task and the connection.
    ///
    /// The client must be explicitly shutdown before dropping otherwise
    /// the connection might still be alive for some time before terminating.
    /// Closing the connection is an async call which cannot be done by the
    /// drop method so it must be done explicitly.
    pub async fn shutdown(&mut self) -> Result<(), std::io::Error> {
        debug!("Abort read task");
        if !self.read_task.is_finished() {
            self.read_task.abort();
        }

        // Cancel heart beat task
        if let Some(ref handle) = self.heartbeat_task.take() {
            if !handle.is_finished() {
                debug!("Abort heartbeat task");
                handle.abort();
            }
        }

        debug!("Shutdown writer");
        let mut writer = self.writer.lock().await;
        writer.shutdown().await
    }

    /// Reconnect with server.
    ///
    /// Make a new connection with server. Use the new read and write halves
    /// to update the shared writer and the read and heartbeat tasks.
    ///
    /// TODO: fix error type
    pub async fn reconnect(&mut self) -> Result<(), Error> {
        let SocketConfig {
            url,
            mode,
            heartbeat,
            suffix,
            handler,
        } = &self.config;
        debug!("Reconnecting client");
        let (reader, new_writer) = Self::tls_connect_with_server(url, *mode).await?;

        debug!("Use new writer end");
        let mut guard = self.writer.lock().await;
        *guard = new_writer;
        drop(guard);

        debug!("Recreate reader and heartbeat task");
        self.read_task = Self::spawn_read_task(reader, handler.clone(), suffix.clone());
        self.heartbeat_task =
            Self::spawn_heartbeat_task(heartbeat.clone(), self.writer.clone(), suffix.clone());
        Ok(())
    }

    /// Check if the client is still connected.
    ///
    /// The client is connected if the read task has not finished. It is expected
    /// that in case of any failure client or server side. The read task will be
    /// shutdown. There might be some delay between the connection being closed
    /// and the client detecting it.
    #[inline]
    #[must_use]
    pub fn is_alive(&self) -> bool {
        !self.read_task.is_finished()
    }
}

impl Drop for SocketClientInner {
    fn drop(&mut self) {
        if !self.read_task.is_finished() {
            self.read_task.abort();
        }

        // Cancel heart beat task
        if let Some(ref handle) = self.heartbeat_task.take() {
            if !handle.is_finished() {
                handle.abort();
            }
        }
    }
}

#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.network")
)]
pub struct SocketClient {
    writer: SharedTcpWriter,
    controller_task: task::JoinHandle<()>,
    disconnect_mode: Arc<Mutex<bool>>,
    suffix: Vec<u8>,
}

impl SocketClient {
    pub async fn connect(
        config: SocketConfig,
        post_connection: Option<PyObject>,
        post_reconnection: Option<PyObject>,
        post_disconnection: Option<PyObject>,
    ) -> Result<Self, Error> {
        let suffix = config.suffix.clone();
        let inner = SocketClientInner::connect_url(config).await?;
        let writer = inner.writer.clone();
        let disconnect_mode = Arc::new(Mutex::new(false));
        let controller_task = Self::spawn_controller_task(
            inner,
            disconnect_mode.clone(),
            post_reconnection,
            post_disconnection,
        );

        if let Some(handler) = post_connection {
            Python::with_gil(|py| match handler.call0(py) {
                Ok(_) => debug!("Called `post_connection` handler"),
                Err(e) => error!("Error calling `post_connection` handler: {e}"),
            });
        }

        Ok(Self {
            writer,
            controller_task,
            disconnect_mode,
            suffix,
        })
    }

    /// Set disconnect mode to true.
    ///
    /// Controller task will periodically check the disconnect mode
    /// and shutdown the client if it is not alive.
    pub async fn disconnect(&self) {
        *self.disconnect_mode.lock().await = true;
    }

    pub async fn send_bytes(&self, data: &[u8]) -> Result<(), std::io::Error> {
        let mut writer = self.writer.lock().await;
        writer.write_all(data).await?;
        writer.write_all(&self.suffix).await
    }

    #[must_use]
    pub fn is_disconnected(&self) -> bool {
        self.controller_task.is_finished()
    }

    fn spawn_controller_task(
        mut inner: SocketClientInner,
        disconnect_mode: Arc<Mutex<bool>>,
        post_reconnection: Option<PyObject>,
        post_disconnection: Option<PyObject>,
    ) -> task::JoinHandle<()> {
        task::spawn(async move {
            let mut disconnect_flag;
            loop {
                sleep(Duration::from_secs(1)).await;

                // Check if client needs to disconnect
                let guard = disconnect_mode.lock().await;
                disconnect_flag = *guard;
                drop(guard);

                match (disconnect_flag, inner.is_alive()) {
                    (false, false) => match inner.reconnect().await {
                        Ok(()) => {
                            debug!("Reconnected successfully");
                            if let Some(ref handler) = post_reconnection {
                                Python::with_gil(|py| match handler.call0(py) {
                                    Ok(_) => debug!("Called `post_reconnection` handler"),
                                    Err(e) => {
                                        error!("Error calling `post_reconnection` handler: {e}");
                                    }
                                });
                            }
                        }
                        Err(e) => {
                            error!("Reconnect failed {e}");
                            break;
                        }
                    },
                    (true, true) => {
                        debug!("Shutting down inner client");
                        match inner.shutdown().await {
                            Ok(()) => debug!("Closed connection"),
                            Err(e) => error!("Error on `shutdown`: {e}"),
                        }

                        if let Some(ref handler) = post_disconnection {
                            Python::with_gil(|py| match handler.call0(py) {
                                Ok(_) => debug!("Called `post_disconnection` handler"),
                                Err(e) => {
                                    error!("Error calling `post_disconnection` handler: {e}");
                                }
                            });
                        }
                        break;
                    }
                    (true, false) => break,
                    _ => (),
                }
            }
        })
    }
}

#[pymethods]
impl SocketClient {
    /// Create a socket client.
    ///
    /// # Safety
    ///
    /// - Throws an Exception if it is unable to make socket connection
    #[staticmethod]
    #[pyo3(name = "connect")]
    fn py_connect(
        config: SocketConfig,
        post_connection: Option<PyObject>,
        post_reconnection: Option<PyObject>,
        post_disconnection: Option<PyObject>,
        py: Python<'_>,
    ) -> PyResult<&PyAny> {
        pyo3_asyncio::tokio::future_into_py(py, async move {
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
    /// The connection is not completely closed the till all references
    /// to the client are gone and the client is dropped.
    ///
    /// # Safety
    ///
    /// - The client should not be used after closing it
    /// - Any auto-reconnect job should be aborted before closing the client
    #[pyo3(name = "disconnect")]
    fn py_disconnect<'py>(slf: PyRef<'_, Self>, py: Python<'py>) -> PyResult<&'py PyAny> {
        let disconnect_mode = slf.disconnect_mode.clone();
        debug!("Setting disconnect mode to true");

        pyo3_asyncio::tokio::future_into_py(py, async move {
            *disconnect_mode.lock().await = true;
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
    /// because the connection disconnected and the client is still alive
    /// and reconnecting. In such cases the send can be retried after some
    /// delay
    #[getter]
    fn is_alive(slf: PyRef<'_, Self>) -> bool {
        !slf.controller_task.is_finished()
    }

    /// Send bytes data to the connection.
    ///
    /// # Safety
    ///
    /// - Throws an Exception if it is not able to send data.
    #[pyo3(name = "send")]
    fn py_send<'py>(
        slf: PyRef<'_, Self>,
        mut data: Vec<u8>,
        py: Python<'py>,
    ) -> PyResult<&'py PyAny> {
        let writer = slf.writer.clone();
        data.extend(&slf.suffix);

        pyo3_asyncio::tokio::future_into_py(py, async move {
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
    use tracing::debug;
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

            // Setup test server
            let handle = task::spawn(async move {
                // keep listening for new connections
                loop {
                    let (mut stream, _) = server.accept().await.unwrap();
                    debug!("socket:test Server accepted connection");

                    // keep receiving messages from connection
                    // and sending them back as it is
                    // if the message contains a close stop receiving messages
                    // and drop the connection
                    task::spawn(async move {
                        let mut buf = Vec::new();
                        loop {
                            let bytes = stream.read_buf(&mut buf).await.unwrap();
                            debug!("socket:test Server received {bytes} bytes");

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
                                        debug!("socket:test Client sent closing message");
                                        return;
                                    } else {
                                        debug!("socket:test Server sending message");
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

        // check that messages were received correctly after reconnecting
        assert_eq!(count_value, N + N);

        // Shutdown client and wait for read task to terminate
        client.disconnect().await;
        sleep(Duration::from_secs(1)).await;
        assert!(client.is_disconnected());
    }
}

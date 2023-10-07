// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use futures_util::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use nautilus_core::python::to_pyruntime_err;
use pyo3::{exceptions::PyException, prelude::*, types::PyBytes, PyObject, Python};
use tokio::{net::TcpStream, sync::Mutex, task, time::sleep};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Error, Message},
    MaybeTlsStream, WebSocketStream,
};
use tracing::{debug, error};

type MessageWriter = SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>;
type SharedMessageWriter =
    Arc<Mutex<SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>>>;
type MessageReader = SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>;

/// `WebSocketClient` connects to a websocket server to read and send messages.
///
/// The client is opinionated about how messages are read and written. It
/// assumes that data can only have one reader but multiple writers.
///
/// The client splits the connection into read and write halves. It moves
/// the read half into a tokio task which keeps receiving messages from the
/// server and calls a handler - a Python function that takes the data
/// as its parameter. It stores the write half in the struct wrapped
/// with an Arc Mutex. This way the client struct can be used to write
/// data to the server from multiple scopes/tasks.
///
/// The client also maintains a heartbeat if given a duration in seconds.
/// It's preferable to set the duration slightly lower - heartbeat more
/// frequently - than the required amount.
struct WebSocketClientInner {
    read_task: task::JoinHandle<()>,
    heartbeat_task: Option<task::JoinHandle<()>>,
    writer: SharedMessageWriter,
    url: String,
    handler: PyObject,
    heartbeat: Option<u64>,
}

impl WebSocketClientInner {
    /// Create an inner websocket client.
    pub async fn connect_url(
        url: &str,
        handler: PyObject,
        heartbeat: Option<u64>,
    ) -> Result<Self, Error> {
        let (writer, reader) = Self::connect_with_server(url).await?;
        let writer = Arc::new(Mutex::new(writer));
        let handler_clone = handler.clone();

        // Keep receiving messages from socket and pass them as arguments to handler
        let read_task = Self::spawn_read_task(reader, handler);

        let heartbeat_task = Self::spawn_heartbeat_task(heartbeat, writer.clone());

        Ok(Self {
            read_task,
            heartbeat_task,
            writer,
            url: url.to_string(),
            handler: handler_clone,
            heartbeat,
        })
    }

    /// Connects with the server creating a tokio-tungstenite websocket stream.
    #[inline]
    pub async fn connect_with_server(url: &str) -> Result<(MessageWriter, MessageReader), Error> {
        connect_async(url).await.map(|resp| resp.0.split())
    }

    /// Optionally spawn a hearbeat task to periodically ping the server.
    pub fn spawn_heartbeat_task(
        heartbeat: Option<u64>,
        writer: SharedMessageWriter,
    ) -> Option<task::JoinHandle<()>> {
        heartbeat.map(|duration| {
            task::spawn(async move {
                let duration = Duration::from_secs(duration);
                loop {
                    sleep(duration).await;
                    debug!("Sending heartbeat");
                    let mut guard = writer.lock().await;
                    match guard.send(Message::Ping(vec![])).await {
                        Ok(()) => debug!("Sent heartbeat"),
                        Err(e) => error!("Failed to send heartbeat: {e}"),
                    }
                }
            })
        })
    }

    /// Keep receiving messages from socket and pass them as arguments to handler.
    pub fn spawn_read_task(mut reader: MessageReader, handler: PyObject) -> task::JoinHandle<()> {
        task::spawn(async move {
            loop {
                debug!("Receiving message");
                match reader.next().await {
                    Some(Ok(Message::Binary(data))) => {
                        debug!("Received binary message");
                        if let Err(e) =
                            Python::with_gil(|py| handler.call1(py, (PyBytes::new(py, &data),)))
                        {
                            error!("Call to handler failed: {e}");
                            break;
                        }
                    }
                    Some(Ok(Message::Text(data))) => {
                        debug!("Received text message");
                        if let Err(e) = Python::with_gil(|py| {
                            handler.call1(py, (PyBytes::new(py, data.as_bytes()),))
                        }) {
                            error!("Call to handler failed: {e}");
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) => {
                        error!("Received close message. Terminating.");
                        break;
                    }
                    Some(Ok(_)) => (),
                    Some(Err(e)) => {
                        error!("Received error message. Terminating. {e}");
                        break;
                    }
                    // Internally tungstenite considers the connection closed when polling
                    // for the next message in the stream returns None.
                    None => {
                        error!("No next message received. Terminating");
                        break;
                    }
                }
            }
        })
    }

    /// Shutdown read and hearbeat task and the connection.
    ///
    /// The client must be explicitly shutdown before dropping otherwise
    /// the connection might still be alive for some time before terminating.
    /// Closing the connection is an async call which cannot be done by the
    /// drop method so it must be done explicitly.
    pub async fn shutdown(&mut self) {
        debug!("Closing connection");

        if !self.read_task.is_finished() {
            self.read_task.abort();
            debug!("Aborted message read task");
        }

        // Cancel heart beat task
        if let Some(ref handle) = self.heartbeat_task.take() {
            if !handle.is_finished() {
                debug!("Aborting heart beat task");
                handle.abort();
            }
        }

        debug!("Closing writer");
        let mut write_half = self.writer.lock().await;
        write_half.close().await.unwrap();
        debug!("Closed connection");
    }

    /// Reconnect with server
    ///
    /// Make a new connection with server. Use the new read and write halves
    /// to update self writer and read and heartbeat tasks.
    pub async fn reconnect(&mut self) -> Result<(), Error> {
        let (new_writer, reader) = Self::connect_with_server(&self.url).await?;
        let mut guard = self.writer.lock().await;
        *guard = new_writer;
        drop(guard);

        self.read_task = Self::spawn_read_task(reader, self.handler.clone());
        self.heartbeat_task = Self::spawn_heartbeat_task(self.heartbeat, self.writer.clone());

        Ok(())
    }

    /// Check if the client is still connected.
    ///
    /// The client is connected if the read task has not finished. It is expected
    /// that in case of any failure client or server side. The read task will be
    /// shutdown or will receive a `Close` frame which will finish it. There
    /// might be some delay between the connection being closed and the client
    /// detecting.
    pub fn is_alive(&self) -> bool {
        !self.read_task.is_finished()
    }
}

impl Drop for WebSocketClientInner {
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
    pyclass(module = "nautilus_trader.core.nautilus_pyo3.network")
)]
pub struct WebSocketClient {
    writer: SharedMessageWriter,
    controller_task: task::JoinHandle<()>,
    disconnect_mode: Arc<Mutex<bool>>,
}

impl WebSocketClient {
    /// Creates a websocket client.
    ///
    /// Creates an inner client and controller task to reconnect or disconnect
    /// the client. Also assumes ownership of writer from inner client
    pub async fn connect(
        url: &str,
        handler: PyObject,
        heartbeat: Option<u64>,
        post_connection: Option<PyObject>,
        post_reconnection: Option<PyObject>,
        post_disconnection: Option<PyObject>,
    ) -> Result<Self, Error> {
        let inner = WebSocketClientInner::connect_url(url, handler, heartbeat).await?;
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
                Ok(_) => debug!("Called post_connection handler"),
                Err(e) => error!("Error calling post_connection handler: {e}"),
            });
        }

        Ok(Self {
            writer,
            controller_task,
            disconnect_mode,
        })
    }

    /// Set disconnect mode to true.
    ///
    /// Controller task will periodically check the disconnect mode
    /// and shutdown the client if it is alive
    pub async fn disconnect(&self) {
        *self.disconnect_mode.lock().await = true;
    }

    pub async fn send_bytes(&self, data: Vec<u8>) -> Result<(), Error> {
        let mut guard = self.writer.lock().await;
        guard.send(Message::Binary(data)).await
    }

    #[must_use]
    pub fn is_disconnected(&self) -> bool {
        self.controller_task.is_finished()
    }

    pub async fn send_close_message(&self) {
        let mut guard = self.writer.lock().await;
        match guard.send(Message::Close(None)).await {
            Ok(()) => debug!("Sent close message"),
            Err(e) => error!("Failed to send message: {e}"),
        }
    }

    fn spawn_controller_task(
        mut inner: WebSocketClientInner,
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
                                    Ok(_) => debug!("Called post_reconnection handler"),
                                    Err(e) => {
                                        error!("Error calling post_reconnection handler: {e}");
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
                        inner.shutdown().await;
                        if let Some(ref handler) = post_disconnection {
                            Python::with_gil(|py| match handler.call0(py) {
                                Ok(_) => debug!("Called post_reconnection handler"),
                                Err(e) => {
                                    error!("Error calling post_reconnection handler: {e}");
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
impl WebSocketClient {
    /// Create a websocket client.
    ///
    /// # Safety
    ///
    /// - Throws an Exception if it is unable to make websocket connection
    #[staticmethod]
    #[pyo3(name = "connect")]
    fn py_connect(
        url: String,
        handler: PyObject,
        heartbeat: Option<u64>,
        post_connection: Option<PyObject>,
        post_reconnection: Option<PyObject>,
        post_disconnection: Option<PyObject>,
        py: Python<'_>,
    ) -> PyResult<&PyAny> {
        pyo3_asyncio::tokio::future_into_py(py, async move {
            Self::connect(
                &url,
                handler,
                heartbeat,
                post_connection,
                post_reconnection,
                post_disconnection,
            )
            .await
            .map_err(|e| {
                PyException::new_err(format!(
                    "Unable to make websocket connection because of error: {e}",
                ))
            })
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
    /// delay.
    #[getter]
    fn is_alive(slf: PyRef<'_, Self>) -> bool {
        !slf.controller_task.is_finished()
    }

    /// Send text data to the connection.
    ///
    /// # Safety
    ///
    /// - Raises PyRuntimeError if not able to send data.
    #[pyo3(name = "send_text")]
    fn py_send_text<'py>(
        slf: PyRef<'_, Self>,
        data: String,
        py: Python<'py>,
    ) -> PyResult<&'py PyAny> {
        let writer = slf.writer.clone();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            let mut guard = writer.lock().await;
            guard
                .send(Message::Text(data))
                .await
                .map_err(to_pyruntime_err)
        })
    }

    /// Send bytes data to the connection.
    ///
    /// # Safety
    ///
    /// - Raises PyRuntimeError if not able to send data.
    #[pyo3(name = "send")]
    fn py_send<'py>(slf: PyRef<'_, Self>, data: Vec<u8>, py: Python<'py>) -> PyResult<&'py PyAny> {
        let writer = slf.writer.clone();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            let mut guard = writer.lock().await;
            guard
                .send(Message::Binary(data))
                .await
                .map_err(to_pyruntime_err)
        })
    }
}

#[cfg(test)]
mod tests {
    use futures_util::{SinkExt, StreamExt};
    use pyo3::{prelude::*, prepare_freethreaded_python};
    use tokio::{
        net::TcpListener,
        task::{self, JoinHandle},
        time::{sleep, Duration},
    };
    use tokio_tungstenite::accept_async;
    use tracing::debug;
    use tracing_test::traced_test;

    use crate::websocket::WebSocketClient;

    struct TestServer {
        task: JoinHandle<()>,
        port: u16,
    }

    impl TestServer {
        async fn setup() -> Self {
            let server = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = TcpListener::local_addr(&server).unwrap().port();

            // Setup test server
            let task = task::spawn(async move {
                // keep accepting connections
                loop {
                    let (conn, _) = server.accept().await.unwrap();
                    let mut websocket = accept_async(conn).await.unwrap();

                    task::spawn(async move {
                        loop {
                            let msg = websocket.next().await.unwrap().unwrap();
                            // We do not want to send back ping/pong messages.
                            if msg.is_binary() || msg.is_text() {
                                websocket.send(msg).await.unwrap();
                            } else if msg.is_close() {
                                if let Err(e) = websocket.close(None).await {
                                    debug!("Connection already closed {e}");
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

        // Initialize test server
        let server = TestServer::setup().await;

        // Create counter class and handler that increments it
        let (counter, handler) = Python::with_gil(|py| {
            let pymod = PyModule::from_code(
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

        let client = WebSocketClient::connect(
            &format!("ws://127.0.0.1:{}", server.port),
            handler.clone(),
            None,
            None,
            None,
            None,
        )
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

        // Shutdown client and wait for read task to terminate
        client.disconnect().await;
        sleep(Duration::from_secs(1)).await;
        assert!(client.is_disconnected());
    }
}

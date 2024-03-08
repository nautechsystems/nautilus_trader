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

use std::{str::FromStr, sync::Arc, time::Duration};

use futures_util::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use hyper::header::HeaderName;
use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use pyo3::{prelude::*, types::PyBytes};
use tokio::{net::TcpStream, sync::Mutex, task, time::sleep};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, http::HeaderValue, Error, Message},
    MaybeTlsStream, WebSocketStream,
};
use tracing::{debug, error};

type MessageWriter = SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>;
type SharedMessageWriter =
    Arc<Mutex<SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>>>;
type MessageReader = SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>;

#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.network")
)]
pub struct WebSocketConfig {
    url: String,
    handler: PyObject,
    headers: Vec<(String, String)>,
    heartbeat: Option<u64>,
    heartbeat_msg: Option<String>,
    ping_handler: Option<PyObject>,
}

#[pymethods]
impl WebSocketConfig {
    #[new]
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
            handler,
            headers,
            heartbeat,
            heartbeat_msg,
            ping_handler,
        }
    }
}

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
    config: WebSocketConfig,
    read_task: task::JoinHandle<()>,
    heartbeat_task: Option<task::JoinHandle<()>>,
    writer: SharedMessageWriter,
}

impl WebSocketClientInner {
    /// Create an inner websocket client.
    pub async fn connect_url(config: WebSocketConfig) -> Result<Self, Error> {
        let WebSocketConfig {
            url,
            handler,
            heartbeat,
            headers,
            heartbeat_msg,
            ping_handler,
        } = &config;
        let (writer, reader) = Self::connect_with_server(url, headers.clone()).await?;
        let writer = Arc::new(Mutex::new(writer));

        // Keep receiving messages from socket and pass them as arguments to handler
        let read_task = Self::spawn_read_task(reader, handler.clone(), ping_handler.clone());
        let heartbeat_task =
            Self::spawn_heartbeat_task(*heartbeat, heartbeat_msg.clone(), writer.clone());

        Ok(Self {
            config,
            read_task,
            heartbeat_task,
            writer,
        })
    }

    /// Connects with the server creating a tokio-tungstenite websocket stream.
    #[inline]
    pub async fn connect_with_server(
        url: &str,
        headers: Vec<(String, String)>,
    ) -> Result<(MessageWriter, MessageReader), Error> {
        let mut request = url.into_client_request()?;
        let req_headers = request.headers_mut();

        // Hacky solution to overcome the new `http` trait bounds
        for (key, val) in headers {
            let header_value = HeaderValue::from_str(&val).unwrap();
            let header_name = HeaderName::from_str(&key).unwrap();
            let header_name_string = header_name.to_string();
            let header_name_str: &'static str = Box::leak(header_name_string.into_boxed_str());
            req_headers.insert(header_name_str, header_value);
        }

        connect_async(request).await.map(|resp| resp.0.split())
    }

    /// Optionally spawn a hearbeat task to periodically ping the server.
    pub fn spawn_heartbeat_task(
        heartbeat: Option<u64>,
        message: Option<String>,
        writer: SharedMessageWriter,
    ) -> Option<task::JoinHandle<()>> {
        debug!("Started task `heartbeat`");
        heartbeat.map(|duration| {
            task::spawn(async move {
                let duration = Duration::from_secs(duration);
                loop {
                    sleep(duration).await;
                    let mut guard = writer.lock().await;
                    let guard_send_response = match message.clone() {
                        Some(msg) => guard.send(Message::Text(msg)).await,
                        None => guard.send(Message::Ping(vec![])).await,
                    };
                    match guard_send_response {
                        Ok(()) => debug!("Sent ping"),
                        Err(e) => error!("Error sending ping: {e}"),
                    }
                }
            })
        })
    }

    /// Keep receiving messages from socket and pass them as arguments to handler.
    pub fn spawn_read_task(
        mut reader: MessageReader,
        handler: PyObject,
        ping_handler: Option<PyObject>,
    ) -> task::JoinHandle<()> {
        debug!("Started task `read`");
        task::spawn(async move {
            loop {
                match reader.next().await {
                    Some(Ok(Message::Binary(data))) => {
                        debug!("Received message <binary>");
                        if let Err(e) =
                            Python::with_gil(|py| handler.call1(py, (PyBytes::new(py, &data),)))
                        {
                            error!("Error calling handler: {e}");
                            break;
                        }
                        continue;
                    }
                    Some(Ok(Message::Text(data))) => {
                        debug!("Received message: {data}");
                        if let Err(e) = Python::with_gil(|py| {
                            handler.call1(py, (PyBytes::new(py, data.as_bytes()),))
                        }) {
                            error!("Error calling handler: {e}");
                            break;
                        }
                        continue;
                    }
                    Some(Ok(Message::Ping(ping))) => {
                        let payload = String::from_utf8(ping.clone()).expect("Invalid payload");
                        debug!("Received ping: {payload}",);
                        if let Some(ref handler) = ping_handler {
                            if let Err(e) =
                                Python::with_gil(|py| handler.call1(py, (PyBytes::new(py, &ping),)))
                            {
                                error!("Error calling handler: {e}");
                                break;
                            }
                        }
                        continue;
                    }
                    Some(Ok(Message::Pong(_))) => {
                        debug!("Received pong");
                    }
                    Some(Ok(Message::Close(_))) => {
                        error!("Received close message - terminating");
                        break;
                    }
                    Some(Ok(_)) => (),
                    Some(Err(e)) => {
                        error!("Received error message - terminating: {e}");
                        break;
                    }
                    // Internally tungstenite considers the connection closed when polling
                    // for the next message in the stream returns None.
                    None => {
                        error!("No message received - terminating");
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
                handle.abort();
                debug!("Aborted heartbeat task");
            }
        }

        debug!("Closing writer");
        let mut write_half = self.writer.lock().await;
        write_half.close().await.unwrap();
        debug!("Closed connection");
    }

    /// Reconnect with server.
    ///
    /// Make a new connection with server. Use the new read and write halves
    /// to update self writer and read and heartbeat tasks.
    pub async fn reconnect(&mut self) -> Result<(), Error> {
        let (new_writer, reader) =
            Self::connect_with_server(&self.config.url, self.config.headers.clone()).await?;
        let mut guard = self.writer.lock().await;
        *guard = new_writer;
        drop(guard);

        self.read_task = Self::spawn_read_task(
            reader,
            self.config.handler.clone(),
            self.config.ping_handler.clone(),
        );
        self.heartbeat_task = Self::spawn_heartbeat_task(
            self.config.heartbeat,
            self.config.heartbeat_msg.clone(),
            self.writer.clone(),
        );

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
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.network")
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
    /// the client. Also assumes ownership of writer from inner client.
    pub async fn connect(
        config: WebSocketConfig,
        post_connection: Option<PyObject>,
        post_reconnection: Option<PyObject>,
        post_disconnection: Option<PyObject>,
    ) -> Result<Self, Error> {
        debug!("Connecting");
        let inner = WebSocketClientInner::connect_url(config).await?;
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
        };

        Ok(Self {
            writer,
            controller_task,
            disconnect_mode,
        })
    }

    #[must_use]
    pub fn is_disconnected(&self) -> bool {
        self.controller_task.is_finished()
    }

    /// Set disconnect mode to true.
    ///
    /// Controller task will periodically check the disconnect mode
    /// and shutdown the client if it is alive
    pub async fn disconnect(&self) {
        debug!("Disconnecting");
        *self.disconnect_mode.lock().await = true;
    }

    pub async fn send_bytes(&self, data: Vec<u8>) -> Result<(), Error> {
        debug!("Sending bytes: {:?}", data);
        let mut guard = self.writer.lock().await;
        guard.send(Message::Binary(data)).await
    }

    pub async fn send_close_message(&self) {
        let mut guard = self.writer.lock().await;
        match guard.send(Message::Close(None)).await {
            Ok(()) => debug!("Sent close message"),
            Err(e) => error!("Error sending close message: {e}"),
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
                        inner.shutdown().await;
                        if let Some(ref handler) = post_disconnection {
                            Python::with_gil(|py| match handler.call0(py) {
                                Ok(_) => debug!("Called `post_reconnection` handler"),
                                Err(e) => {
                                    error!("Error calling `post_reconnection` handler: {e}");
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
    /// Check if the client is still alive.
    ///
    /// Even if the connection is disconnected the client will still be alive
    /// and trying to reconnect. Only when reconnect fails the client will
    /// terminate.
    ///
    /// This is particularly useful for checking why a `send` failed. It could
    /// because the connection disconnected and the client is still alive
    /// and reconnecting. In such cases the send can be retried after some
    /// delay.
    #[getter]
    fn is_alive(slf: PyRef<'_, Self>) -> bool {
        !slf.controller_task.is_finished()
    }

    /// Create a websocket client.
    ///
    /// # Safety
    ///
    /// - Throws an Exception if it is unable to make websocket connection
    #[staticmethod]
    #[pyo3(name = "connect")]
    fn py_connect(
        config: WebSocketConfig,
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
        pyo3_asyncio::tokio::future_into_py(py, async move {
            *disconnect_mode.lock().await = true;
            Ok(())
        })
    }

    /// Send bytes data to the server.
    ///
    /// # Safety
    ///
    /// - Raises PyRuntimeError if not able to send data.
    #[pyo3(name = "send")]
    fn py_send<'py>(slf: PyRef<'_, Self>, data: Vec<u8>, py: Python<'py>) -> PyResult<&'py PyAny> {
        debug!("Sending bytes {:?}", data);
        let writer = slf.writer.clone();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            let mut guard = writer.lock().await;
            guard
                .send(Message::Binary(data))
                .await
                .map_err(to_pyruntime_err)
        })
    }

    /// Send text data to the server.
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
        debug!("Sending text: {}", data);
        let writer = slf.writer.clone();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            let mut guard = writer.lock().await;
            guard
                .send(Message::Text(data))
                .await
                .map_err(to_pyruntime_err)
        })
    }

    /// Send pong bytes data to the server.
    ///
    /// # Safety
    ///
    /// - Raises PyRuntimeError if not able to send data.
    #[pyo3(name = "send_pong")]
    fn py_send_pong<'py>(
        slf: PyRef<'_, Self>,
        data: Vec<u8>,
        py: Python<'py>,
    ) -> PyResult<&'py PyAny> {
        let data_str = String::from_utf8(data.clone()).map_err(to_pyvalue_err)?;
        debug!("Sending pong: {}", data_str);
        let writer = slf.writer.clone();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            let mut guard = writer.lock().await;
            guard
                .send(Message::Pong(data))
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
    use tokio_tungstenite::{
        accept_hdr_async,
        tungstenite::{
            handshake::server::{self, Callback},
            http::HeaderValue,
        },
    };
    use tracing::debug;
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

            // Setup test server
            let task = task::spawn(async move {
                // keep accepting connections
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
        let header_key = "hello-custom-key".to_string();
        let header_value = "hello-custom-value".to_string();

        // Initialize test server
        let server = TestServer::setup(header_key.clone(), header_value.clone()).await;

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

        let config = WebSocketConfig::py_new(
            format!("ws://127.0.0.1:{}", server.port),
            handler.clone(),
            vec![(header_key, header_value)],
            None,
            None,
            None,
        );
        let client = WebSocketClient::connect(config, None, None, None)
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

    #[tokio::test]
    #[traced_test]
    async fn message_ping_test() {
        prepare_freethreaded_python();

        let header_key = "hello-custom-key".to_string();
        let header_value = "hello-custom-value".to_string();

        let (checker, handler) = Python::with_gil(|py| {
            let pymod = PyModule::from_code(
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
            handler.clone(),
            vec![(header_key, header_value)],
            Some(1),
            Some("heartbeat message".to_string()),
            None,
        );
        let client = WebSocketClient::connect(config, None, None, None)
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

        // Shutdown client and wait for read task to terminate
        client.disconnect().await;
        sleep(Duration::from_secs(1)).await;
        assert!(client.is_disconnected());
    }
}

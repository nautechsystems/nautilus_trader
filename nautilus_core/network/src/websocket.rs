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

//! High-performance WebSocket client with automatic reconnection and state management.
//!
//! **Key features**:
//! - Connection state tracking (ACTIVE/RECONNECTING/CLOSED)
//! - Synchronized reconnection with backoff
//! - Clean shutdown sequence
//! - Split read/write architecture
//! - Python callback integration
//!
//! **Design**:
//! - Single reader, multiple writer model
//! - Read half runs in dedicated task
//! - Write half protected by `Arc<Mutex>`
//! - Controller task manages lifecycle

use std::{
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        Arc,
    },
    time::Duration,
};

use futures_util::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use http::HeaderName;
use nautilus_cryptography::providers::install_cryptographic_provider;
use pyo3::{prelude::*, types::PyBytes};
use tokio::{net::TcpStream, sync::Mutex, task, time::sleep};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, http::HeaderValue, Error, Message},
    MaybeTlsStream, WebSocketStream,
};

use crate::ratelimiter::{clock::MonotonicClock, quota::Quota, RateLimiter};
type MessageWriter = SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>;
type SharedMessageWriter =
    Arc<Mutex<SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>>>;
pub type MessageReader = SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>;

/// Connection state for the WebSocket client
///
/// - ACTIVE: Normal operation, all tasks running
/// - RECONNECTING: In process of reconnecting, tasks paused
/// - CLOSED: Connection terminated, cleanup in progress
///
/// Connection state transitions:
/// ACTIVE <-> RECONNECTING: During reconnection attempts
/// ACTIVE/RECONNECTING -> CLOSED: Only when controller task terminates
const CONNECTION_ACTIVE: u8 = 0;
const CONNECTION_RECONNECTING: u8 = 1;
const CONNECTION_CLOSED: u8 = 2;

#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.network")
)]
pub struct WebSocketConfig {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub handler: Option<Arc<PyObject>>,
    pub heartbeat: Option<u64>,
    pub heartbeat_msg: Option<String>,
    pub ping_handler: Option<Arc<PyObject>>,
    pub max_reconnection_tries: Option<u64>,
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
    read_task: Option<task::JoinHandle<()>>,
    heartbeat_task: Option<task::JoinHandle<()>>,
    writer: SharedMessageWriter,
    reconnection_lock: Arc<Mutex<()>>,
    connection_state: Arc<AtomicU8>,
}

impl WebSocketClientInner {
    /// Create an inner websocket client.
    pub async fn connect_url(config: WebSocketConfig) -> Result<Self, Error> {
        install_cryptographic_provider();

        #[allow(unused_variables)]
        let WebSocketConfig {
            url,
            handler,
            heartbeat,
            headers,
            heartbeat_msg,
            ping_handler,
            max_reconnection_tries,
        } = &config;
        let (writer, reader) = Self::connect_with_server(url, headers.clone()).await?;
        let writer = Arc::new(Mutex::new(writer));

        let connection_state = Arc::new(AtomicU8::new(CONNECTION_ACTIVE));
        let reconnection_lock = Arc::new(Mutex::new(()));

        // Only spawn read task if handler is provided
        let read_task = handler
            .as_ref()
            .map(|handler| Self::spawn_read_task(reader, handler.clone(), ping_handler.clone()));

        let heartbeat_task = Self::spawn_heartbeat_task(
            connection_state.clone(),
            *heartbeat,
            heartbeat_msg.clone(),
            writer.clone(),
        );

        Ok(Self {
            config,
            read_task,
            heartbeat_task,
            writer,
            reconnection_lock,
            connection_state,
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

        let mut header_names: Vec<HeaderName> = Vec::new();
        for (key, val) in headers {
            let header_value = HeaderValue::from_str(&val)?;
            let header_name: HeaderName = key.parse()?;
            header_names.push(header_name.clone());
            req_headers.insert(header_name, header_value);
        }

        connect_async(request).await.map(|resp| resp.0.split())
    }

    /// Optionally spawn a hearbeat task to periodically ping the server.
    pub fn spawn_heartbeat_task(
        connection_state: Arc<AtomicU8>,
        heartbeat: Option<u64>,
        message: Option<String>,
        writer: SharedMessageWriter,
    ) -> Option<task::JoinHandle<()>> {
        tracing::debug!("Started task 'heartbeat'");

        heartbeat.map(|duration| {
            task::spawn(async move {
                let duration = Duration::from_secs(duration);
                while connection_state.load(Ordering::SeqCst) == CONNECTION_ACTIVE {
                    sleep(duration).await;
                    if connection_state.load(Ordering::SeqCst) != CONNECTION_ACTIVE {
                        break;
                    }
                    let mut guard = writer.lock().await;
                    // Only send if still active
                    if connection_state.load(Ordering::SeqCst) == CONNECTION_ACTIVE {
                        let guard_send_response = match message.clone() {
                            Some(msg) => guard.send(Message::Text(msg.into())).await,
                            None => guard.send(Message::Ping(vec![].into())).await,
                        };

                        match guard_send_response {
                            Ok(()) => tracing::trace!("Sent ping"),
                            Err(e) => tracing::error!("Error sending ping: {e}"),
                        }
                    }
                }
            })
        })
    }

    fn spawn_read_task(
        mut reader: MessageReader,
        handler: Arc<PyObject>,
        ping_handler: Option<Arc<PyObject>>,
    ) -> task::JoinHandle<()> {
        tracing::debug!("Started task 'read'");

        task::spawn(async move {
            loop {
                match reader.next().await {
                    Some(Ok(Message::Binary(data))) => {
                        tracing::trace!("Received message <binary> {} bytes", data.len());
                        if let Err(e) =
                            Python::with_gil(|py| handler.call1(py, (PyBytes::new(py, &data),)))
                        {
                            tracing::error!("Error calling handler: {e}");
                            break;
                        }
                        continue;
                    }
                    Some(Ok(Message::Text(data))) => {
                        tracing::trace!("Received message: {data}");
                        if let Err(e) = Python::with_gil(|py| {
                            handler.call1(py, (PyBytes::new(py, data.as_bytes()),))
                        }) {
                            tracing::error!("Error calling handler: {e}");
                            break;
                        }
                        continue;
                    }
                    Some(Ok(Message::Ping(ping))) => {
                        tracing::trace!("Received ping: {ping:?}",);
                        if let Some(ref handler) = ping_handler {
                            if let Err(e) =
                                Python::with_gil(|py| handler.call1(py, (PyBytes::new(py, &ping),)))
                            {
                                tracing::error!("Error calling handler: {e}");
                                break;
                            }
                        }
                        continue;
                    }
                    Some(Ok(Message::Pong(_))) => {
                        tracing::trace!("Received pong");
                    }
                    Some(Ok(Message::Close(_))) => {
                        tracing::error!("Received close message - terminating");
                        break;
                    }
                    Some(Ok(_)) => (),
                    Some(Err(e)) => {
                        tracing::error!("Received error message - terminating: {e}");
                        break;
                    }
                    // Internally tungstenite considers the connection closed when polling
                    // for the next message in the stream returns None.
                    None => {
                        tracing::error!("No message received - terminating");
                        break;
                    }
                }
            }
        })
    }

    /// Reconnect with server.
    ///
    /// Make a new connection with server. Use the new read and write halves
    /// to update self writer and read and heartbeat tasks.
    pub async fn reconnect(&mut self) -> Result<(), Error> {
        tracing::debug!("Reconnecting client");

        // TODO: Expose reconnect timeout as config option
        let timeout = Duration::from_secs(30);
        tokio::time::timeout(timeout, async {
            let state_guard = {
                let guard = self.reconnection_lock.lock().await;
                self.connection_state
                    .store(CONNECTION_RECONNECTING, Ordering::SeqCst);
                guard
            };

            sleep(Duration::from_millis(100)).await;

            shutdown(
                self.read_task.take(),
                self.heartbeat_task.take(),
                self.writer.clone(),
            )
            .await;

            let (new_writer, reader) =
                Self::connect_with_server(&self.config.url, self.config.headers.clone()).await?;

            {
                let mut guard = self.writer.lock().await;
                *guard = new_writer;
                drop(guard);
            }

            // Spawn new read task
            if let Some(ref handler) = self.config.handler {
                self.read_task = Some(Self::spawn_read_task(
                    reader,
                    handler.clone(),
                    self.config.ping_handler.clone(),
                ));
            }

            // Spawn new heartbeat task
            self.heartbeat_task = Self::spawn_heartbeat_task(
                self.connection_state.clone(),
                self.config.heartbeat,
                self.config.heartbeat_msg.clone(),
                self.writer.clone(),
            );

            self.connection_state
                .store(CONNECTION_ACTIVE, Ordering::SeqCst);
            drop(state_guard);

            tracing::debug!("Reconnect succeeded");
            Ok(())
        })
        .await
        .map_err(|_| {
            Error::Io(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("reconnection timed out after {}s", timeout.as_secs()),
            ))
        })?
    }

    /// Check if the client is still connected.
    ///
    /// The client is connected if the read task has not finished. It is expected
    /// that in case of any failure client or server side. The read task will be
    /// shutdown or will receive a `Close` frame which will finish it. There
    /// might be some delay between the connection being closed and the client
    /// detecting.
    #[inline]
    #[must_use]
    pub fn is_alive(&self) -> bool {
        match &self.read_task {
            Some(read_task) => !read_task.is_finished(),
            None => true, // Stream is being used directly
        }
    }
}

/// Shutdown websocket connection.
///
/// Performs orderly WebSocket shutdown:
/// 1. Sends close frame to server
/// 2. Waits briefly for frame delivery
/// 3. Aborts read/heartbeat tasks
/// 4. Closes underlying connection
///
/// This sequence ensures proper protocol compliance and clean resource cleanup.
async fn shutdown(
    read_task: Option<task::JoinHandle<()>>,
    heartbeat_task: Option<task::JoinHandle<()>>,
    writer: SharedMessageWriter,
) {
    tracing::debug!("Closing");

    let timeout = Duration::from_secs(5);
    if tokio::time::timeout(timeout, async {
        // Send close frame first
        let mut write_half = writer.lock().await;
        if let Err(e) = write_half.send(Message::Close(None)).await {
            tracing::error!("Error sending close frame: {e}");
        }
        drop(write_half);

        sleep(Duration::from_millis(100)).await;

        // Abort tasks
        if let Some(task) = read_task {
            if !task.is_finished() {
                task.abort();
                tracing::debug!("Aborted read task");
            }
        }
        if let Some(task) = heartbeat_task {
            if !task.is_finished() {
                task.abort();
                tracing::debug!("Aborted heartbeat task");
            }
        }

        // Final close of writer
        let mut write_half = writer.lock().await;
        if let Err(e) = write_half.close().await {
            tracing::error!("Error closing writer: {e}");
        }
    })
    .await
    .is_err()
    {
        tracing::error!("Shutdown timed out after {}s", timeout.as_secs());
    }

    tracing::debug!("Closed");
}

impl Drop for WebSocketClientInner {
    fn drop(&mut self) {
        if let Some(ref read_task) = self.read_task.take() {
            if !read_task.is_finished() {
                read_task.abort();
            }
        }

        // Cancel heart beat task
        if let Some(ref handle) = self.heartbeat_task.take() {
            if !handle.is_finished() {
                handle.abort();
            }
        }
    }
}

/// WebSocket client with automatic reconnection.
///
/// Handles connection state, Python callbacks, and rate limiting.
/// See module docs for architecture details.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.network")
)]
pub struct WebSocketClient {
    pub(crate) writer: SharedMessageWriter,
    pub(crate) controller_task: task::JoinHandle<()>,
    pub(crate) rate_limiter: Arc<RateLimiter<String, MonotonicClock>>,
    pub(crate) disconnect_mode: Arc<AtomicBool>,
}

impl WebSocketClient {
    /// Creates a websocket client that returns a stream for reading messages.
    pub async fn connect_stream(
        url: String,
        headers: Vec<(String, String)>,
        heartbeat: Option<u64>,
        heartbeat_msg: Option<String>,
        max_reconnection_tries: Option<u64>,
        keyed_quotas: Vec<(String, Quota)>,
        default_quota: Option<Quota>,
    ) -> Result<(MessageReader, Self), Error> {
        let (ws_stream, _) = connect_async(url.clone().into_client_request()?).await?;
        let (writer, reader) = ws_stream.split();
        let writer = Arc::new(Mutex::new(writer));

        // Create config with minimal no-op Python handler so we incrementally
        // move towards a more Rust-native approach.
        let config = {
            WebSocketConfig {
                url,
                handler: None,
                headers,
                heartbeat,
                heartbeat_msg,
                ping_handler: None,
                max_reconnection_tries,
            }
        };

        let disconnect_mode = Arc::new(AtomicBool::new(false));
        let rate_limiter = Arc::new(RateLimiter::new_with_quota(default_quota, keyed_quotas));

        let inner = WebSocketClientInner::connect_url(config).await?;
        let controller_task = Self::spawn_controller_task(
            inner,
            disconnect_mode.clone(),
            None, // no post_reconnection
            None, // no post_disconnection
            max_reconnection_tries,
        );

        Ok((
            reader,
            Self {
                writer: writer.clone(),
                controller_task,
                rate_limiter,
                disconnect_mode,
            },
        ))
    }

    /// Creates a websocket client.
    ///
    /// Creates an inner client and controller task to reconnect or disconnect
    /// the client. Also assumes ownership of writer from inner client.
    pub async fn connect(
        config: WebSocketConfig,
        post_connection: Option<PyObject>,
        post_reconnection: Option<PyObject>,
        post_disconnection: Option<PyObject>,
        keyed_quotas: Vec<(String, Quota)>,
        default_quota: Option<Quota>,
    ) -> Result<Self, Error> {
        tracing::debug!("Connecting");
        let inner = WebSocketClientInner::connect_url(config.clone()).await?;
        let writer = inner.writer.clone();
        let disconnect_mode = Arc::new(AtomicBool::new(false));

        let controller_task = Self::spawn_controller_task(
            inner,
            disconnect_mode.clone(),
            post_reconnection,
            post_disconnection,
            config.max_reconnection_tries,
        );
        let rate_limiter = Arc::new(RateLimiter::new_with_quota(default_quota, keyed_quotas));

        if let Some(handler) = post_connection {
            Python::with_gil(|py| match handler.call0(py) {
                Ok(_) => tracing::debug!("Called `post_connection` handler"),
                Err(e) => tracing::error!("Error calling `post_connection` handler: {e}"),
            });
        };

        Ok(Self {
            writer,
            controller_task,
            rate_limiter,
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
        tracing::debug!("Disconnecting");
        self.disconnect_mode.store(true, Ordering::SeqCst);

        match tokio::time::timeout(Duration::from_secs(5), async {
            while !self.is_disconnected() {
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        {
            Ok(()) => {
                tracing::debug!("Controller task finished");
            }
            Err(_) => {
                tracing::error!("Timeout waiting for controller task to finish");
            }
        }
    }

    pub async fn send_text(&self, data: String, keys: Option<Vec<String>>) -> Result<(), Error> {
        self.rate_limiter.await_keys_ready(keys).await;
        tracing::trace!("Sending text: {data:?}");
        let mut guard = self.writer.lock().await;
        guard.send(Message::Text(data.into())).await
    }

    pub async fn send_bytes(&self, data: Vec<u8>, keys: Option<Vec<String>>) -> Result<(), Error> {
        self.rate_limiter.await_keys_ready(keys).await;
        tracing::trace!("Sending bytes: {data:?}");
        let mut guard = self.writer.lock().await;
        guard.send(Message::Binary(data.into())).await
    }

    pub async fn send_close_message(&self) {
        let mut guard = self.writer.lock().await;
        match guard.send(Message::Close(None)).await {
            Ok(()) => tracing::debug!("Sent close message"),
            Err(e) => tracing::error!("Error sending close message: {e}"),
        }
    }

    fn spawn_controller_task(
        mut inner: WebSocketClientInner,
        disconnect_mode: Arc<AtomicBool>,
        post_reconnection: Option<PyObject>,
        post_disconnection: Option<PyObject>,
        max_reconnection_tries: Option<u64>,
    ) -> task::JoinHandle<()> {
        task::spawn(async move {
            let check_interval = Duration::from_millis(100);
            let retry_interval = Duration::from_millis(1000);
            let mut retry_counter: u64 = 0;

            loop {
                sleep(check_interval).await;

                // Check if client needs to disconnect
                let disconnect = disconnect_mode.load(Ordering::SeqCst);
                match (disconnect, inner.is_alive()) {
                    (false, false) => match inner.reconnect().await {
                        Ok(()) => {
                            tracing::debug!("Reconnected successfully");
                            retry_counter = 0;

                            if let Some(ref handler) = post_reconnection {
                                Python::with_gil(|py| match handler.call0(py) {
                                    Ok(_) => tracing::debug!("Called `post_reconnection` handler"),
                                    Err(e) => {
                                        tracing::error!(
                                            "Error calling `post_reconnection` handler: {e}"
                                        );
                                    }
                                });
                            }
                        }
                        Err(e) => {
                            retry_counter += 1;

                            if let Some(max) = max_reconnection_tries {
                                tracing::warn!("Reconnect failed {e}. Retry {retry_counter}/{max}");

                                if retry_counter >= max {
                                    tracing::error!("Reached max reconnection tries");
                                    break;
                                }
                            } else {
                                tracing::warn!(
                                    "Reconnect failed {e}. Retry {retry_counter} (infinite)"
                                );
                            }

                            sleep(retry_interval).await;
                        }
                    },
                    (true, true) => {
                        shutdown(
                            inner.read_task.take(),
                            inner.heartbeat_task.take(),
                            inner.writer.clone(),
                        )
                        .await;
                        if let Some(ref handler) = post_disconnection {
                            Python::with_gil(|py| match handler.call0(py) {
                                Ok(_) => tracing::debug!("Called `post_disconnection` handler"),
                                Err(e) => {
                                    tracing::error!(
                                        "Error calling `post_disconnection` handler: {e}"
                                    );
                                }
                            });
                        }
                        break;
                    }
                    // Close the heartbeat task on disconnect if the connection is already closed
                    (true, false) => {
                        tracing::debug!("Inner client is disconnected");
                        tracing::debug!("Shutting down inner client to clean up running tasks");
                        shutdown(
                            inner.read_task.take(),
                            inner.heartbeat_task.take(),
                            inner.writer.clone(),
                        )
                        .await;
                    }
                    _ => (),
                }
            }
            inner
                .connection_state
                .store(CONNECTION_CLOSED, Ordering::SeqCst);
        })
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
#[cfg(target_os = "linux")] // Only run network tests on Linux (CI stability)
mod tests {
    use std::{num::NonZeroU32, sync::Arc};

    use futures_util::{SinkExt, StreamExt};
    use tokio::{
        net::TcpListener,
        task::{self, JoinHandle},
    };
    use tokio_tungstenite::{
        accept_hdr_async,
        tungstenite::{
            handshake::server::{self, Callback},
            http::HeaderValue,
        },
    };

    use crate::{
        ratelimiter::quota::Quota,
        websocket::{WebSocketClient, WebSocketConfig},
    };

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
        async fn setup() -> Self {
            let server = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = TcpListener::local_addr(&server).unwrap().port();

            let header_key = "test".to_string();
            let header_value = "test".to_string();

            let test_call_back = TestCallback {
                key: header_key,
                value: HeaderValue::from_str(&header_value).unwrap(),
            };

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
                                    tracing::debug!("Forcibly closing from server side");
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

    async fn setup_test_client(port: u16) -> WebSocketClient {
        let config = WebSocketConfig {
            url: format!("ws://127.0.0.1:{port}"),
            headers: vec![("test".into(), "test".into())],
            handler: None,
            heartbeat: None,
            heartbeat_msg: None,
            ping_handler: None,
            max_reconnection_tries: None,
        };
        WebSocketClient::connect(config, None, None, None, vec![], None)
            .await
            .expect("Failed to connect")
    }

    #[tokio::test]
    async fn test_websocket_basic() {
        let server = TestServer::setup().await;
        let client = setup_test_client(server.port).await;

        assert!(!client.is_disconnected());

        client.disconnect().await;
        assert!(client.is_disconnected());
    }

    #[tokio::test]
    async fn test_websocket_heartbeat() {
        let server = TestServer::setup().await;
        let client = setup_test_client(server.port).await;

        // Wait ~3s => server should see multiple "ping"
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        // Cleanup
        client.disconnect().await;
        assert!(client.is_disconnected());
    }

    #[tokio::test]
    async fn test_websocket_reconnect_exhausted() {
        let config = WebSocketConfig {
            url: "ws://127.0.0.1:9997".into(), // <-- No server
            headers: vec![],
            handler: None,
            heartbeat: None,
            heartbeat_msg: None,
            ping_handler: None,
            max_reconnection_tries: Some(2),
        };
        let res = WebSocketClient::connect(config, None, None, None, vec![], None).await;
        assert!(res.is_err(), "Should fail quickly with no server");
    }

    #[tokio::test]
    async fn test_websocket_forced_close_reconnect() {
        let server = TestServer::setup().await;
        let client = setup_test_client(server.port).await;

        // 1) Send normal message
        client.send_text("Hello".into(), None).await.unwrap();

        // 2) Trigger forced close from server
        client.send_text("close-now".into(), None).await.unwrap();

        // 3) Wait a bit => read loop sees close => reconnect
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        // Confirm not disconnected
        assert!(!client.is_disconnected());

        // Cleanup
        client.disconnect().await;
        assert!(client.is_disconnected());
    }

    #[tokio::test]
    async fn test_rate_limiter() {
        let server = TestServer::setup().await;
        let quota = Quota::per_second(NonZeroU32::new(2).unwrap());

        let config = WebSocketConfig {
            url: format!("ws://127.0.0.1:{}", server.port),
            headers: vec![("test".into(), "test".into())],
            handler: None,
            heartbeat: None,
            heartbeat_msg: None,
            ping_handler: None,
            max_reconnection_tries: None,
        };

        let client = WebSocketClient::connect(
            config,
            None,
            None,
            None,
            vec![("default".into(), quota)],
            None,
        )
        .await
        .unwrap();

        // First 2 should succeed
        client.send_text("test1".into(), None).await.unwrap();
        client.send_text("test2".into(), None).await.unwrap();

        // Third should error
        client.send_text("test3".into(), None).await.unwrap();

        // Cleanup
        client.disconnect().await;
        assert!(client.is_disconnected());
    }

    #[tokio::test]
    async fn test_concurrent_writers() {
        let server = TestServer::setup().await;
        let client = Arc::new(setup_test_client(server.port).await);

        let mut handles = vec![];
        for i in 0..10 {
            let client = client.clone();
            handles.push(task::spawn(async move {
                client.send_text(format!("test{i}"), None).await.unwrap();
            }));
        }

        for handle in handles {
            handle.await.unwrap();
        }

        // Cleanup
        client.disconnect().await;
        assert!(client.is_disconnected());
    }
}

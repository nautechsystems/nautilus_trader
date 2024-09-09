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

//! A high-performance WebSocket client implementation.
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use futures_util::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use hyper::header::HeaderName;
use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use pyo3::{prelude::*, types::PyBytes};
use rustls::crypto::{aws_lc_rs, CryptoProvider};
use tokio::{net::TcpStream, sync::Mutex, task, time::sleep};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, http::HeaderValue, Error, Message},
    MaybeTlsStream, WebSocketStream,
};

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
    pub url: String,
    pub handler: PyObject,
    pub headers: Vec<(String, String)>,
    pub heartbeat: Option<u64>,
    pub heartbeat_msg: Option<String>,
    pub ping_handler: Option<PyObject>,
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
        if CryptoProvider::get_default().is_none() {
            aws_lc_rs::default_provider()
                .install_default()
                .expect("Error installing crypto provider");
        }

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
            use http::header::HeaderName;
            let header_name: HeaderName = key.parse().unwrap();
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
        tracing::debug!("Started task 'heartbeat'");
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
                        Ok(()) => tracing::trace!("Sent ping"),
                        Err(e) => tracing::error!("Error sending ping: {e}"),
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
        tracing::debug!("Started task 'read'");
        task::spawn(async move {
            loop {
                match reader.next().await {
                    Some(Ok(Message::Binary(data))) => {
                        tracing::trace!("Received message <binary>");
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
                        let payload = String::from_utf8(ping.clone()).expect("Invalid payload");
                        tracing::trace!("Received ping: {payload}",);
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

    /// Shutdown read and hearbeat task and the connection.
    ///
    /// The client must be explicitly shutdown before dropping otherwise
    /// the connection might still be alive for some time before terminating.
    /// Closing the connection is an async call which cannot be done by the
    /// drop method so it must be done explicitly.
    pub async fn shutdown(&mut self) {
        tracing::debug!("Closing connection");

        if !self.read_task.is_finished() {
            self.read_task.abort();
            tracing::debug!("Aborted message read task");
        }

        // Cancel heart beat task
        if let Some(ref handle) = self.heartbeat_task.take() {
            if !handle.is_finished() {
                handle.abort();
                tracing::debug!("Aborted heartbeat task");
            }
        }

        tracing::debug!("Closing writer");
        let mut write_half = self.writer.lock().await;
        write_half.close().await.unwrap();
        tracing::debug!("Closed connection");
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
    #[inline]
    #[must_use]
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
    pub(crate) writer: SharedMessageWriter,
    pub(crate) controller_task: task::JoinHandle<()>,
    pub(crate) disconnect_mode: Arc<AtomicBool>,
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
        tracing::debug!("Connecting");
        let inner = WebSocketClientInner::connect_url(config).await?;
        let writer = inner.writer.clone();
        let disconnect_mode = Arc::new(AtomicBool::new(false));

        let controller_task = Self::spawn_controller_task(
            inner,
            disconnect_mode.clone(),
            post_reconnection,
            post_disconnection,
        );

        if let Some(handler) = post_connection {
            Python::with_gil(|py| match handler.call0(py) {
                Ok(_) => tracing::debug!("Called `post_connection` handler"),
                Err(e) => tracing::error!("Error calling `post_connection` handler: {e}"),
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
        tracing::debug!("Disconnecting");
        self.disconnect_mode.store(true, Ordering::SeqCst);

        match tokio::time::timeout(Duration::from_secs(5), async {
            while !self.is_disconnected() {
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        {
            Ok(_) => {
                tracing::debug!("Controller task finished");
            }
            Err(_) => {
                tracing::error!("Timeout waiting for controller task to finish");
            }
        }
    }

    pub async fn send_bytes(&self, data: Vec<u8>) -> Result<(), Error> {
        tracing::trace!("Sending bytes: {:?}", data);
        let mut guard = self.writer.lock().await;
        guard.send(Message::Binary(data)).await
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
    ) -> task::JoinHandle<()> {
        task::spawn(async move {
            loop {
                sleep(Duration::from_millis(100)).await;

                // Check if client needs to disconnect
                let disconnected = disconnect_mode.load(Ordering::SeqCst);
                match (disconnected, inner.is_alive()) {
                    (false, false) => match inner.reconnect().await {
                        Ok(()) => {
                            tracing::debug!("Reconnected successfully");
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
                            tracing::error!("Reconnect failed {e}");
                            break;
                        }
                    },
                    (true, true) => {
                        tracing::debug!("Shutting down inner client");
                        inner.shutdown().await;
                        if let Some(ref handler) = post_disconnection {
                            Python::with_gil(|py| match handler.call0(py) {
                                Ok(_) => tracing::debug!("Called `post_reconnection` handler"),
                                Err(e) => {
                                    tracing::error!(
                                        "Error calling `post_reconnection` handler: {e}"
                                    );
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

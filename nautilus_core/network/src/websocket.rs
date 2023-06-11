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

use std::io;
use std::sync::Arc;
use std::time::Duration;

use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt};
use pyo3::prelude::*;
use pyo3::types::PyBytes;
use pyo3::{PyObject, Python};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::task;
use tokio::time::sleep;
use tokio_tungstenite::tungstenite::{Error, Message};
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use tracing::debug;

/// WebSocketClient connects to a websocket server to read and send messages.
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
#[pyclass]
pub struct WebSocketClient {
    pub read_task: task::JoinHandle<io::Result<()>>,
    pub heartbeat_task: Option<task::JoinHandle<()>>,
    write_mutex: Arc<Mutex<SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>>>,
}

impl WebSocketClient {
    pub async fn connect(
        url: &str,
        handler: PyObject,
        heartbeat: Option<u64>,
    ) -> Result<Self, Error> {
        let (stream, _) = connect_async(url).await?;
        let (write_half, mut read_half) = stream.split();
        let write_mutex = Arc::new(Mutex::new(write_half));

        // Keep receiving messages from socket and pass them as arguments to handler
        let read_task = task::spawn(async move {
            loop {
                debug!("websocket: Receiving message");
                match read_half.next().await {
                    Some(Ok(Message::Binary(bytes))) => {
                        debug!("websocket: Received binary message");
                        Python::with_gil(|py| handler.call1(py, (PyBytes::new(py, &bytes),)))
                            .unwrap();
                    }
                    Some(Ok(Message::Text(data))) => {
                        debug!("websocket: Received text message");
                        Python::with_gil(|py| {
                            handler.call1(py, (PyBytes::new(py, data.as_bytes()),))
                        })
                        .unwrap();
                    }
                    // TODO: log closing
                    Some(Ok(Message::Close(_))) => {
                        debug!("websocket: Received close message. Terminating.");
                        break;
                    }
                    Some(Ok(_)) => (),
                    // TODO: log error
                    Some(Err(err)) => {
                        debug!("websocket: Received error message. Terminating. {err}");
                        break;
                    }
                    // Internally tungstenite considers the connection closed when polling
                    // for the next message in the stream returns None.
                    None => {
                        debug!("websocket: No next message received. Terminating");
                        break;
                    }
                }
            }
            Ok(())
        });

        let heartbeat_task = heartbeat.map(|duration| {
            let heartbeat_writer = write_mutex.clone();
            task::spawn(async move {
                loop {
                    sleep(Duration::from_secs(duration)).await;
                    debug!("websocket: Sending heartbeat");
                    let mut write_half = heartbeat_writer.lock().await;
                    write_half.send(Message::Ping(vec![])).await.unwrap();
                    debug!("websocket: Sent heartbeat");
                }
            })
        });

        Ok(Self {
            read_task,
            heartbeat_task,
            write_mutex,
        })
    }

    pub async fn send(&self, data: Vec<u8>) {
        let mut write_half = self.write_mutex.lock().await;
        write_half.send(Message::Binary(data)).await.unwrap();
    }

    /// Shutdown read and hearbeat task and the connection
    ///
    /// The client must be explicitly shutdown before dropping otherwise
    /// the connection might still be alive for some time before terminating.
    /// Closing the connection is an async call which cannot be done by the
    /// drop method so it must be done explicitly.
    pub async fn shutdown(&mut self) {
        debug!("websocket: Closing connection");

        if !self.read_task.is_finished() {
            self.read_task.abort();
            debug!("websocket: Aborted message read task");
        }

        // Cancel heart beat task
        if let Some(ref handle) = self.heartbeat_task.take() {
            if !handle.is_finished() {
                debug!("websocket: Aborting heart beat task");
                handle.abort();
            }
        }

        debug!("websocket: Closing writer");
        let mut write_half = self.write_mutex.lock().await;
        write_half.close().await.unwrap();
        debug!("websocket: Closed connection");
    }

    // Checks if the client is still connected
    pub fn connection_is_alive(&self) -> bool {
        !self.read_task.is_finished()
    }
}

impl Drop for WebSocketClient {
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

#[pymethods]
impl WebSocketClient {
    #[staticmethod]
    fn connect_url(
        url: String,
        handler: PyObject,
        heartbeat: Option<u64>,
        py: Python<'_>,
    ) -> PyResult<&PyAny> {
        pyo3_asyncio::tokio::future_into_py(py, async move {
            Ok(WebSocketClient::connect(&url, handler, heartbeat)
                .await
                .unwrap())
        })
    }

    /// Send bytes data to the connection.
    fn send_bytes<'py>(
        slf: PyRef<'_, Self>,
        data: Vec<u8>,
        py: Python<'py>,
    ) -> PyResult<&'py PyAny> {
        let write_half = slf.write_mutex.clone();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            debug!("websocket: Sending message");
            let mut write_half = write_half.lock().await;
            write_half.send(Message::Binary(data)).await.unwrap();
            debug!("websocket: Sent message");
            Ok(())
        })
    }

    /// Closes the client heart beat and reader task.
    ///
    /// The connection is not completely closed the till all references
    /// to the client are gone and the client is dropped.
    ///
    /// #Safety
    /// - The client should not be used after closing it
    /// - Any auto-reconnect job should be aborted before closing the client
    fn close<'py>(mut slf: PyRefMut<'_, Self>, py: Python<'py>) -> PyResult<&'py PyAny> {
        debug!("websocket: Closing connection");

        if !slf.read_task.is_finished() {
            slf.read_task.abort();
            debug!("websocket: Aborted message read task");
        }

        // Cancel heart beat task
        if let Some(ref handle) = slf.heartbeat_task.take() {
            if !handle.is_finished() {
                debug!("websocket: Aborting heart beat task");
                handle.abort();
            }
        }

        // Cannot directly call `shutdown` method because it requires
        // an &mut and slf cannot be transferred to a on async move closure
        let write_half = slf.write_mutex.clone();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            let mut write_half = write_half.lock().await;
            write_half.close().await.unwrap();
            debug!("websocket: Closed writer");
            Ok(())
        })
    }

    /// Check if the client is still connected
    ///
    /// The client is connected if the read task has not finished. It is expected
    /// that in case of any failure client or server side. The read task will be
    /// shutdown or will receive a `Close` frame which will finish it. There
    /// might be some delay between the connection being closed and the client
    /// detecting.
    fn is_connected(slf: PyRef<'_, Self>) -> bool {
        slf.connection_is_alive()
    }
}

#[cfg(test)]
mod tests {
    use futures_util::{SinkExt, StreamExt};
    use pyo3::{prelude::*, prepare_freethreaded_python};
    use tokio::{
        net::TcpListener,
        task,
        time::{sleep, Duration},
    };
    use tokio_tungstenite::accept_async;
    use tracing_test::traced_test;

    use super::WebSocketClient;

    struct TestServer {
        port: u16,
    }

    impl TestServer {
        async fn basic_client_test() -> Self {
            let server = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = TcpListener::local_addr(&server).unwrap().port();

            // Setup test server
            task::spawn(async move {
                let (conn, _) = server.accept().await.unwrap();
                let mut websocket = accept_async(conn).await.unwrap();

                loop {
                    let msg = websocket.next().await.unwrap().unwrap();
                    // We do not want to send back ping/pong messages.
                    if msg.is_binary() || msg.is_text() {
                        websocket.send(msg).await.unwrap();
                    } else if msg.is_close() {
                        if let Err(err) = websocket.close(None).await {
                            println!("Connection already closed {err}");
                        };
                        break;
                    }
                }
            });

            Self { port }
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

        let mut client = WebSocketClient::connect(
            &format!("ws://127.0.0.1:{}", server.port),
            handler.clone(),
            None,
        )
        .await
        .unwrap();

        // Check that websocket read task is running
        assert!(client.connection_is_alive());

        // Send messages that increment the count
        for _ in 0..N {
            client.send(b"ping".to_vec()).await;
        }

        sleep(Duration::from_secs(1)).await;
        // Shutdown client and wait for read task to terminate
        client.shutdown().await;

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
    }
}

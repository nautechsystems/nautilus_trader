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

use fastwebsockets::{self, FragmentCollector, Frame, OpCode, Payload, Role, WebSocket};
use pyo3::prelude::*;
use pyo3::types::PyBytes;
use pyo3::{PyObject, Python};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::task;
use tokio::time::sleep;
use tracing::{event, Level};

/// WebSocketClient connects to a websocket server to read and send messages
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
    pub read_task: Option<task::JoinHandle<io::Result<()>>>,
    pub heartbeat_task: Option<task::JoinHandle<()>>,
    inner: Arc<Mutex<FragmentCollector<TcpStream>>>,
}

impl WebSocketClient {
    pub async fn connect(
        url: &str,
        handler: PyObject,
        heartbeat: Option<u64>,
    ) -> Result<Self, io::Error> {
        let stream = TcpStream::connect(url).await?;
        let ws = WebSocket::after_handshake(stream, Role::Client);
        let inner = Arc::new(Mutex::new(FragmentCollector::new(ws)));
        let reader = inner.clone();

        // Keep receiving messages from socket and pass them as arguments to handler
        let read_task = Some(task::spawn(async move {
            loop {
                event!(Level::DEBUG, "websocket: Receiving message");
                let mut reader = reader.lock().await;
                match reader.read_frame().await {
                    Ok(ref mut frame) => match frame.opcode {
                        OpCode::Text | OpCode::Binary => {
                            event!(Level::DEBUG, "websocket: Received binary message");
                            Python::with_gil(|py| {
                                handler
                                    .call1(py, (PyBytes::new(py, frame.payload.to_mut()),))
                                    .unwrap();
                            });
                        }
                        op => {
                            event!(Level::DEBUG, "websocket: Received message of type {:?}", op);
                        }
                    },
                    Err(err) => {
                        event!(
                            Level::DEBUG,
                            "websocket: Received error message. Terminating. {}",
                            err
                        );
                        break;
                    }
                }
            }
            Ok(())
        }));

        let heartbeat_task = heartbeat.map(|duration| {
            let heartbeat_writer = inner.clone();
            task::spawn(async move {
                loop {
                    sleep(Duration::from_secs(duration)).await;
                    event!(Level::DEBUG, "websocket: Sending heartbeat");
                    let mut writer = heartbeat_writer.lock().await;
                    writer
                        .write_frame(Frame::new(true, OpCode::Ping, None, Payload::Borrowed(&[])))
                        .await
                        .unwrap();
                    event!(Level::DEBUG, "websocket: Sent heartbeat");
                }
            })
        });

        Ok(Self {
            read_task,
            heartbeat_task,
            inner,
        })
    }

    pub async fn send(&self, data: Vec<u8>) {
        let mut writer = self.inner.lock().await;
        writer
            .write_frame(Frame::binary(Payload::from(data)))
            .await
            .unwrap();
    }

    pub fn shutdown(&mut self) {
        event!(Level::DEBUG, "websocket: closing connection");
        // Cancel reading task
        if let Some(ref handle) = self.read_task.take() {
            handle.abort();
            event!(Level::DEBUG, "websocket: Aborted message read task");
        }

        // Cancel heart beat task
        if let Some(ref handle) = self.heartbeat_task.take() {
            handle.abort();
            event!(Level::DEBUG, "websocket: Aborted heart beat task");
        }
    }

    pub fn check_read_task(&self) -> bool {
        self.read_task
            .as_ref()
            .map_or(false, |handle| !handle.is_finished())
    }
}

impl Drop for WebSocketClient {
    fn drop(&mut self) {
        self.shutdown();
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

    /// Send bytes data to the connection
    fn send_bytes<'py>(
        slf: PyRef<'_, Self>,
        data: Vec<u8>,
        py: Python<'py>,
    ) -> PyResult<&'py PyAny> {
        let inner = slf.inner.clone();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            event!(Level::DEBUG, "websocket: Sending message");
            let mut writer = inner.lock().await;
            writer
                .write_frame(Frame::binary(Payload::from(data)))
                .await
                .unwrap();
            event!(Level::DEBUG, "websocket: Sent message");
            Ok(())
        })
    }

    /// Closes the client heart beat and reader task
    ///
    /// The connection is not completely closed the till all references
    /// to the client are gone and the client is dropped.
    ///
    /// #Safety
    /// - The client should not be used after closing it
    /// - Any auto-reconnect job should be aborted before closing the client
    fn close(mut slf: PyRefMut<'_, Self>) {
        slf.shutdown()
    }

    /// Check if the client is still connected
    ///
    /// The client is connected if the read task has not finished. It is expected
    /// that in case of any failure client or server side. The read task will be
    /// shutdown or will receive a `Close` frame which will finish it. There
    /// might be some delay between the connection being closed and the client
    /// detecting.
    ///
    /// Internally tungstenite considers the connection closed when polling
    /// for the next message in the stream returns None.
    fn is_connected(slf: PyRef<'_, Self>) -> bool {
        slf.check_read_task()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use fastwebsockets::{FragmentCollector, OpCode, Role, WebSocket};
    use pyo3::{prelude::*, prepare_freethreaded_python};
    use tokio::{
        net::TcpListener,
        task::{self, JoinHandle},
        time::sleep,
    };
    use tracing::{event, Level};
    use tracing_test::traced_test;

    use super::WebSocketClient;

    struct TestServer {
        handle: JoinHandle<()>,
        port: u16,
    }

    impl TestServer {
        fn shutdown(&self) {
            self.handle.abort();
        }

        async fn basic_client_test() -> Self {
            let server = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = TcpListener::local_addr(&server).unwrap().port();
            event!(
                Level::DEBUG,
                "websocket:test Create tcp listener for test server",
            );

            // Setup test server
            let handle = task::spawn(async move {
                let (stream, _) = server.accept().await.unwrap();
                let ws = WebSocket::after_handshake(stream, Role::Server);
                let mut server = FragmentCollector::new(ws);
                event!(Level::DEBUG, "websocket:test Started websocket server",);

                loop {
                    match server.read_frame().await {
                        Ok(frame) => match frame.opcode {
                            OpCode::Binary | OpCode::Text => {
                                server.write_frame(frame).await.unwrap()
                            }
                            _ => (),
                        },
                        Err(err) => {
                            event!(
                                Level::DEBUG,
                                "websocket:test Closing test server because of error: {}",
                                err
                            );
                            break;
                        }
                    }
                }
            });

            Self { handle, port }
        }
    }

    #[tokio::test]
    #[traced_test]
    async fn basic_client_test() {
        prepare_freethreaded_python();

        const N: usize = 10;

        // Initialize test server
        event!(Level::DEBUG, "websocket:test Starting test server");
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

        let mut client =
            WebSocketClient::connect(&format!("127.0.0.1:{}", server.port), handler.clone(), None)
                .await
                .unwrap();

        // Check that websocket read task is running
        let task_running = client
            .read_task
            .as_ref()
            .map_or(false, |handle| !handle.is_finished());
        assert!(task_running);

        // Send messages that increment the count
        for _ in 0..N {
            client.send(b"ping".to_vec()).await;
        }

        // let messages be sent and received
        sleep(Duration::from_secs(1)).await;

        // Shutdown client and server wait for read task to terminate
        client.shutdown();
        server.shutdown();

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

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

use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt};
use pyo3::prelude::*;
use pyo3::types::PyBytes;
use pyo3::{PyObject, Python};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::task;
use tokio_tungstenite::tungstenite::{Error, Message};
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

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
#[pyclass]
pub struct WebSocketClient {
    pub read_task: Option<task::JoinHandle<io::Result<()>>>,
    write_mutex: Arc<Mutex<SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>>>,
}

impl WebSocketClient {
    pub async fn connect(url: &str, handler: PyObject) -> Result<Self, Error> {
        let (stream, _) = connect_async(url).await?;
        let (write_half, mut read_half) = stream.split();
        let write_mutex = Arc::new(Mutex::new(write_half));

        // Keep receiving messages from socket and pass them as arguments to handler
        let read_task = Some(task::spawn(async move {
            loop {
                match read_half.next().await {
                    Some(Ok(Message::Binary(bytes))) => {
                        Python::with_gil(|py| handler.call1(py, (PyBytes::new(py, &bytes),)))
                            .unwrap();
                    }
                    Some(Ok(Message::Text(data))) => {
                        Python::with_gil(|py| {
                            handler.call1(py, (PyBytes::new(py, data.as_bytes()),))
                        })
                        .unwrap();
                    }
                    // TODO: log closing
                    Some(Ok(Message::Close(_))) => break,
                    Some(Ok(_)) => (),
                    // TODO: log error
                    Some(Err(_)) => break,
                    // TODO: break on no next item or not. Probably yes
                    None => (),
                }
            }
            Ok(())
        }));

        Ok(Self {
            read_task,
            write_mutex,
        })
    }

    pub async fn send(&self, data: Vec<u8>) {
        let mut write_half = self.write_mutex.lock().await;
        write_half.send(Message::Binary(data)).await.unwrap();
    }

    pub async fn shutdown(mut self) -> Option<task::JoinHandle<Result<(), io::Error>>> {
        let mut write_half = self.write_mutex.lock().await;
        write_half.close().await.unwrap();
        self.read_task.take()
    }
}

impl Drop for WebSocketClient {
    fn drop(&mut self) {
        // TODO: should this close write end before dropping?
        // cancel reading task
        if let Some(ref handle) = self.read_task {
            handle.abort();
        }
    }
}

#[pymethods]
impl WebSocketClient {
    #[staticmethod]
    fn connect_url(url: String, handler: PyObject, py: Python<'_>) -> PyResult<&PyAny> {
        pyo3_asyncio::tokio::future_into_py(py, async move {
            Ok(WebSocketClient::connect(&url, handler).await.unwrap())
        })
    }

    fn send_bytes<'py>(
        slf: PyRef<'_, Self>,
        data: Vec<u8>,
        py: Python<'py>,
    ) -> PyResult<&'py PyAny> {
        let write_half = slf.write_mutex.clone();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            let mut write_half = write_half.lock().await;
            write_half.send(Message::Binary(data)).await.unwrap();
            Ok(())
        })
    }

    fn close<'py>(slf: PyRef<'_, Self>, py: Python<'py>) -> PyResult<&'py PyAny> {
        // Cancel reading task
        if let Some(ref handle) = slf.read_task {
            handle.abort();
        }

        let write_half = slf.write_mutex.clone();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            let mut write_half = write_half.lock().await;
            write_half.close().await.unwrap();
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{net::TcpListener, thread};

    use pyo3::{prelude::*, prepare_freethreaded_python};
    use tokio_tungstenite::tungstenite::accept;

    use super::WebSocketClient;

    struct TestServer {
        port: u16,
    }

    impl TestServer {
        fn basic_client_test() -> Self {
            let server = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = TcpListener::local_addr(&server).unwrap().port();

            // Setup test server
            thread::spawn(move || {
                let conn = server.incoming().next().unwrap();
                let mut websocket = accept(conn.unwrap()).unwrap();

                loop {
                    let msg = websocket.read_message().unwrap();

                    // We do not want to send back ping/pong messages
                    if msg.is_binary() || msg.is_text() {
                        websocket.write_message(msg).unwrap();
                    } else if msg.is_close() {
                        if let Err(err) = websocket.close(None) {
                            println!("Connection already closed {}", err);
                        };
                        break;
                    }
                }
            });

            TestServer { port }
        }
    }

    #[tokio::test]
    async fn basic_client_test() {
        const N: usize = 10;

        // Initialize test server
        let server = TestServer::basic_client_test();

        prepare_freethreaded_python();

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

        let client =
            WebSocketClient::connect(&format!("ws://127.0.0.1:{}", server.port), handler.clone())
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
            client.send("ping".to_string().into_bytes()).await;
        }

        // Shutdown client and wait for read task to terminate
        let handle = client.shutdown().await.unwrap();
        handle.await.unwrap().unwrap();

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

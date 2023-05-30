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

use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt};
use pyo3::prelude::*;
use pyo3::{PyObject, Python};
use std::io;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::task;
use tokio_tungstenite::tungstenite::{Error, Message};
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

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
#[pyclass]
pub struct WebSocketClient {
    read_task: Option<task::JoinHandle<io::Result<()>>>,
    write_mutex: Arc<Mutex<SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>>>,
}

impl WebSocketClient {
    pub async fn connect(url: &str, handler: PyObject) -> Result<Self, Error> {
        let (stream, _) = connect_async(url).await?;
        let (write_half, mut read_half) = stream.split();
        let write_mutex = Arc::new(Mutex::new(write_half));

        // keep receiving messages from socket
        // pass them as arguments to handler
        let read_task = Some(task::spawn(async move {
            loop {
                match read_half.next().await {
                    Some(Ok(Message::Binary(bytes))) => {
                        Python::with_gil(|py| handler.call1(py, (bytes,)));
                    }
                    Some(Ok(Message::Text(data))) => {
                        let bytes = data.into_bytes();
                        Python::with_gil(|py| handler.call1(py, (bytes,)));
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
}

#[pymethods]
impl WebSocketClient {
    #[staticmethod]
    fn connect_url<'py>(url: String, handler: PyObject, py: Python<'py>) -> PyResult<&'py PyAny> {
        pyo3_asyncio::tokio::future_into_py(py, async move {
            Ok(WebSocketClient::connect(&url, handler).await.unwrap())
        })
    }

    fn send<'py>(slf: PyRef<'_, Self>, data: Vec<u8>, py: Python<'py>) -> PyResult<&'py PyAny> {
        let write_half = slf.write_mutex.clone();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            let mut write_half = write_half.lock().await;
            write_half.send(Message::Binary(data)).await.unwrap();
            Ok(())
        })
    }

    fn close<'py>(slf: PyRef<'_, Self>, py: Python<'py>) -> PyResult<&'py PyAny> {
        // cancel reading task
        if let Some(ref handle) = slf.read_task {
            handle.abort();
        }

        let write_half = slf.write_mutex.clone();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            let mut write_half = write_half.lock().await;
            write_half.close();
            Ok(())
        })
    }
}

impl Drop for WebSocketClient {
    fn drop(&mut self) {
        // cancel reading task
        if let Some(ref handle) = self.read_task {
            handle.abort();
        }

        // close write half
        let mut write_half = self.write_mutex.blocking_lock();
        write_half.close();
        drop(write_half);
    }
}

#[cfg(test)]
mod tests {
    use std::{net::TcpListener, thread};

    use pyo3::{impl_::pyfunction, prepare_freethreaded_python, pyfunction, wrap_pyfunction};
    use tokio_tungstenite::tungstenite::accept;

    #[test]
    fn test_client() {
        thread::spawn(|| {
            let server = TcpListener::bind("127.0.0.1:9001").unwrap();
            let conn = server.incoming().next().unwrap();
            let mut websocket = accept(conn.unwrap()).unwrap();

            // echo 10 messages before shutting down
            for _ in 0..10 {
                let msg = websocket.read_message().unwrap();

                // We do not want to send back ping/pong messages.
                if msg.is_binary() || msg.is_text() {
                    websocket.write_message(msg).unwrap();
                }
            }
        });

        // let client = WebSocketClient::connect("ws://127.0.0.1:9001");

        // for _ in 0..10 {
        //     client.send(tungstenite::Message::Text("ping".to_string()));
        //     assert_eq!(client.recv(), Some("ping".to_string().into_bytes()));
        // }
    }

    #[test]
    fn test_close() {
        thread::spawn(|| {
            let server = TcpListener::bind("127.0.0.1:9001").unwrap();
            let conn = server.incoming().next().unwrap();
            let mut websocket = accept(conn.unwrap()).unwrap();

            let msg = websocket.read_message().unwrap();

            // We do not want to send back ping/pong messages.
            if msg.is_binary() || msg.is_text() {
                websocket.close(None).unwrap();
            }
        });

        // let client = WebSocketClient::connect("ws://127.0.0.1:9001");

        // client.send(tungstenite::Message::Text("ping".to_string()));
        // assert_eq!(client.recv(), None);
    }
}

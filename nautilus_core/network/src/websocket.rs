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

use std::{net::TcpStream, panic, sync::Mutex};

use pyo3::prelude::*;
use tungstenite::{connect, stream::MaybeTlsStream, Message, WebSocket};

#[pyclass]
pub struct WebSocketClient {
    stream: Mutex<WebSocket<MaybeTlsStream<TcpStream>>>,
}

impl WebSocketClient {
    pub fn connect(url: &str) -> Self {
        match connect(url) {
            Ok((stream, _resp)) => WebSocketClient {
                stream: Mutex::new(stream),
            },
            Err(err) => {
                panic!("Cannot connect to websocket server {}", err);
            }
        }
    }

    pub fn close(&self) {
        if let Ok(mut stream) = self.stream.lock() {
            if let Err(err) = stream.close(None) {
                panic!("Connection could not be closed {}", err);
            };
        }
    }

    pub fn send(&self, msg: Message) {
        // TODO: Will block till a message is received
        if let Ok(mut stream) = self.stream.lock() {
            if let Err(err) = stream.write_message(msg) {
                panic!("Message could not be sent {}", err);
            };
        }
    }

    pub fn recv(&self) -> Option<Vec<u8>> {
        if let Ok(mut stream) = self.stream.lock() {
            // TODO: will wait forever if the server doesn't send any message
            match stream.read_message() {
                Ok(Message::Text(txt)) => Some(txt.into_bytes()),
                Ok(Message::Binary(data)) => Some(data),
                Ok(Message::Close(_)) => None,
                // TODO: other messages should be filtered but returns an empty list
                Ok(_) => Some(vec![]),
                Err(err) => {
                    panic!("Error with stream {}", err);
                }
            }
        } else {
            None
        }
    }
}

#[pymethods]
impl WebSocketClient {
    #[new]
    fn new(url: String) -> Self {
        WebSocketClient::connect(&url)
    }

    fn close_conn(slf: PyRef<'_, Self>) {
        slf.close();
    }

    pub fn send_bytes(slf: PyRef<'_, Self>, data: Vec<u8>) {
        slf.send(Message::Binary(data));
    }

    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    /// Each iteration returns a chunk of values read from the parquet file.
    fn __next__(slf: PyRef<'_, Self>) -> Option<PyObject> {
        slf.recv()
            .map(|data| Python::with_gil(|py| data.into_py(py)))
    }
}

#[cfg(test)]
mod tests {
    use std::{net::TcpListener, thread};

    use tungstenite::{accept, client};

    use super::WebSocketClient;

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

        let client = WebSocketClient::connect("ws://127.0.0.1:9001");

        for _ in 0..10 {
            client.send(tungstenite::Message::Text("ping".to_string()));
            assert_eq!(client.recv(), Some("ping".to_string().into_bytes()));
        }
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

        let client = WebSocketClient::connect("ws://127.0.0.1:9001");

        client.send(tungstenite::Message::Text("ping".to_string()));
        assert_eq!(client.recv(), None);
    }
}

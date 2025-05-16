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

// Under development
#![allow(dead_code)]

use std::net::SocketAddr;

use futures::{SinkExt, StreamExt};
use tokio::{task, time::Duration};

pub struct NegativeStreamServer {
    task: tokio::task::JoinHandle<()>,
    port: u16,
    pub address: SocketAddr,
}

impl NegativeStreamServer {
    pub async fn setup() -> Self {
        let server = tokio::net::TcpListener::bind("127.0.0.1:0".to_string())
            .await
            .unwrap();
        let port = server.local_addr().unwrap().port();
        let address = server.local_addr().unwrap();

        let task = task::spawn(async move {
            let (conn, _) = server.accept().await.unwrap();
            let websocket = tokio_tungstenite::accept_async(conn).await.unwrap();
            let (mut sender, mut receiver) = websocket.split();

            // Create a counter for negative values
            let counter = std::sync::Arc::new(std::sync::atomic::AtomicI32::new(0));
            let counter_clone = counter.clone();
            let counter_clone_2 = counter;

            // Task to send negative numbers every second
            let sender_task = task::spawn(async move {
                loop {
                    let value = counter_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let message = tokio_tungstenite::tungstenite::protocol::Message::Text(
                        format!("{}", -value).into(),
                    );

                    if let Err(err) = sender.send(message).await {
                        eprintln!("Error sending message: {err}");
                        break;
                    }

                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            });

            // Task to handle incoming messages
            task::spawn(async move {
                while let Some(Ok(msg)) = receiver.next().await {
                    if let tokio_tungstenite::tungstenite::protocol::Message::Text(txt) = msg {
                        if txt == "SKIP" {
                            counter_clone_2.fetch_add(5, std::sync::atomic::Ordering::SeqCst);
                        } else if txt == "STOP" {
                            break;
                        }
                    }
                }

                // Cancel the sender task when we're done
                sender_task.abort();
            });
        });

        Self {
            task,
            port,
            address,
        }
    }
}

impl Drop for NegativeStreamServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

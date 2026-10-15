// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

use std::sync::{Mutex, Once};

use futures_util::{SinkExt, StreamExt};
use log::{Level, LevelFilter, Log, Metadata, Record};
use tokio_tungstenite::{
    WebSocketStream, accept_hdr_async,
    tungstenite::{
        Message,
        handshake::server::{Request, Response},
    },
};

pub(super) const REQUEST_TARGET: &str = "/URL_SECRET_PATH?token=URL_SECRET_QUERY";
static LOGS: Mutex<Vec<(Level, String)>> = Mutex::new(Vec::new());
static CAPTURE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static INSTALL: Once = Once::new();

struct Capture;
impl Log for Capture {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.target().starts_with("nautilus_")
    }

    fn log(&self, record: &Record<'_>) {
        if self.enabled(record.metadata()) {
            LOGS.lock()
                .unwrap()
                .push((record.level(), record.args().to_string()));
        }
    }

    fn flush(&self) {}
}

pub(super) async fn capture_logs() -> tokio::sync::MutexGuard<'static, ()> {
    let guard = CAPTURE_LOCK.lock().await;
    INSTALL.call_once(|| log::set_logger(&Capture).unwrap());
    log::set_max_level(LevelFilter::Trace);
    LOGS.lock().unwrap().clear();
    log::debug!(target: "nautilus_logging_test", "Log capture control");
    assert_eq!(
        messages()
            .into_iter()
            .filter(|(_, message)| message == "Log capture control")
            .collect::<Vec<_>>(),
        vec![(Level::Debug, "Log capture control".to_string())]
    );
    LOGS.lock().unwrap().clear();
    guard
}

pub(super) fn messages() -> Vec<(Level, String)> {
    LOGS.lock().unwrap().clone()
}

pub(super) struct WebSocketServer {
    pub(super) url: String,
    pub(super) origin: String,
    pub(super) requests: tokio::sync::oneshot::Receiver<Vec<String>>,
    pub(super) task: tokio::task::JoinHandle<()>,
}

pub(super) async fn start_server(connections: usize, status: Option<u16>) -> WebSocketServer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, requests) = tokio::sync::oneshot::channel();

    let task = tokio::spawn(async move {
        let mut captured = Vec::new();
        let mut sockets = tokio::task::JoinSet::new();

        for _ in 0..connections {
            let (stream, _) = listener.accept().await.unwrap();

            #[allow(
                clippy::result_large_err,
                reason = "tungstenite fixes the handshake callback error type"
            )]
            let socket = accept_hdr_async(stream, |request: &Request, response: Response| {
                captured.push(request.uri().to_string());

                if let Some(status) = status {
                    return Err(Response::builder()
                        .status(status)
                        .body(Some("URL_SECRET_BODY".to_string()))
                        .unwrap());
                }

                Ok(response)
            })
            .await;

            let socket = match socket {
                Ok(socket) => socket,
                Err(_) => {
                    assert!(status.is_some());
                    continue;
                }
            };

            sockets.spawn(serve_socket(socket));
        }

        tx.send(captured).unwrap();

        while sockets.join_next().await.is_some() {}
    });

    WebSocketServer {
        url: format!("ws://URL_SECRET_USER:URL_SECRET_PASSWORD@{addr}{REQUEST_TARGET}"),
        origin: format!("ws://{addr}"),
        requests,
        task,
    }
}

async fn serve_socket(mut socket: WebSocketStream<tokio::net::TcpStream>) {
    while let Some(Ok(message)) = socket.next().await {
        if let Message::Text(text) = &message
            && let Ok(request) = serde_json::from_str::<serde_json::Value>(text)
            && request["method"] == "public/login"
        {
            let response = serde_json::json!({"jsonrpc":"2.0","id":request["id"],"error":{"code":-32000,"message":"URL_SECRET_PROVIDER_MESSAGE","data":{"token":"URL_SECRET_PROVIDER_DATA"}}});
            socket
                .send(Message::Text(response.to_string().into()))
                .await
                .unwrap();
        }

        if matches!(message, Message::Close(_)) {
            let _ = socket.flush().await;
            break;
        }

        if matches!(message, Message::Ping(_)) {
            socket.flush().await.unwrap();
        }
    }
}

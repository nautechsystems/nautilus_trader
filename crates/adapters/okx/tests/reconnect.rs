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

use std::{
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use futures_util::{SinkExt, StreamExt};
use nautilus_live::{SocketControl, SocketReconnectRegistry, SocketReconnectRequestOutcome};
use nautilus_model::identifiers::ClientId;
use nautilus_network::websocket::TransportBackend;
use nautilus_okx::{common::enums::OKXInstrumentType, websocket::client::OKXWebSocketClient};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

static LOGGER: ReconnectSchedule = ReconnectSchedule;
static INSTALL_WINDOW: AtomicBool = AtomicBool::new(false);
static RECONNECTS: AtomicUsize = AtomicUsize::new(0);
static AUTH_ATTEMPTS: AtomicUsize = AtomicUsize::new(0);
static SUBSCRIPTION: Gate = Gate::new();
static SECOND_NOTIFICATION: Gate = Gate::new();
static INSTALL: Gate = Gate::new();

struct ReconnectSchedule;

impl log::Log for ReconnectSchedule {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        let text = record.args().to_string();
        let install_window = INSTALL_WINDOW.load(Ordering::Acquire);
        if install_window && text == "Writer confirmed socket update: epoch=2" {
            INSTALL.wait();
        }

        if !record.target().starts_with("nautilus_okx::websocket") {
            return;
        }

        if text.starts_with("Subscribed: channel=orders,")
            && !SUBSCRIPTION.entered.load(Ordering::Acquire)
        {
            SUBSCRIPTION.wait();
        }

        if text == "Received WebSocket reconnection signal"
            && RECONNECTS.fetch_add(1, Ordering::AcqRel) == 1
            && !install_window
        {
            SECOND_NOTIFICATION.wait();
        }

        if text == "Re-authenticating after reconnection" {
            AUTH_ATTEMPTS.fetch_add(1, Ordering::AcqRel);
        }
    }

    fn flush(&self) {}
}

struct Gate {
    open: Mutex<bool>,
    changed: Condvar,
    entered: AtomicBool,
}

impl Gate {
    const fn new() -> Self {
        Self {
            open: Mutex::new(false),
            changed: Condvar::new(),
            entered: AtomicBool::new(false),
        }
    }

    fn reset(&self) {
        *self.open.lock().unwrap() = false;
        self.entered.store(false, Ordering::Release);
    }

    fn wait(&self) {
        self.entered.store(true, Ordering::Release);
        // Hand off the worker so spawned authentication tasks can leave its local queue
        tokio::task::block_in_place(|| {
            let _wait = self
                .changed
                .wait_timeout_while(self.open.lock().unwrap(), Duration::from_secs(10), |open| {
                    !*open
                })
                .unwrap();
        });
    }

    fn release(&self) {
        *self.open.lock().unwrap() = true;
        self.changed.notify_all();
    }
}

struct ReleaseGates;

impl Drop for ReleaseGates {
    fn drop(&mut self) {
        SUBSCRIPTION.release();
        SECOND_NOTIFICATION.release();
        INSTALL.release();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn buffered_reconnects_authenticate_the_surviving_connection() {
    log::set_logger(&LOGGER).unwrap();
    log::set_max_level(log::LevelFilter::Debug);

    for backend in [TransportBackend::Tungstenite, TransportBackend::Sockudo] {
        for install_window in [false, true] {
            for gate in [&SUBSCRIPTION, &SECOND_NOTIFICATION, &INSTALL] {
                gate.reset();
            }

            let _release = ReleaseGates;
            INSTALL_WINDOW.store(install_window, Ordering::Release);
            RECONNECTS.store(0, Ordering::Release);
            AUTH_ATTEMPTS.store(0, Ordering::Release);
            verify_reconnect_login(backend, install_window).await;
        }
    }
}

async fn verify_reconnect_login(backend: TransportBackend, install_window: bool) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let connections = Arc::new(AtomicUsize::new(0));
    let logins = Arc::new(Mutex::new(Vec::new()));

    let server = {
        let connections = Arc::clone(&connections);
        let logins = Arc::clone(&logins);
        tokio::spawn(serve_logins(listener, connections, logins))
    };

    let registry = SocketReconnectRegistry::default();
    let client_id = ClientId::from("BUFFERED-RECONNECT");
    let mut ws_client = OKXWebSocketClient::new(
        Some(format!("ws://{address}")),
        Some("key".into()),
        Some("secret".into()),
        Some("passphrase".into()),
        None,
        Some(30),
        Some(10),
        backend,
        None,
    )
    .unwrap()
    .with_socket_control(SocketControl::with_registry(
        client_id, None, "private", &registry,
    ));

    ws_client.connect().await.unwrap();
    ws_client
        .subscribe_orders(OKXInstrumentType::Spot)
        .await
        .unwrap();
    wait_until(|| SUBSCRIPTION.entered.load(Ordering::Acquire)).await;
    let reconnect = registry.handle(client_id, Ustr::from("private")).unwrap();
    assert_eq!(
        reconnect.request_reconnect(),
        SocketReconnectRequestOutcome::Accepted
    );
    wait_until(|| connections.load(Ordering::Acquire) == 2 && ws_client.is_active()).await;
    assert_eq!(
        reconnect.request_reconnect(),
        SocketReconnectRequestOutcome::Accepted
    );

    if install_window {
        wait_until(|| INSTALL.entered.load(Ordering::Acquire)).await;
        assert!(!ws_client.is_active());
        SUBSCRIPTION.release();
        wait_until(|| RECONNECTS.load(Ordering::Acquire) == 1).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        let premature_attempts = AUTH_ATTEMPTS.load(Ordering::Acquire);
        INSTALL.release();
        assert_eq!(premature_attempts, 0);
    } else {
        wait_until(|| connections.load(Ordering::Acquire) == 3 && ws_client.is_active()).await;
        SUBSCRIPTION.release();
        wait_until(|| {
            SECOND_NOTIFICATION.entered.load(Ordering::Acquire)
                && logins.lock().unwrap().contains(&2)
        })
        .await;

        SECOND_NOTIFICATION.release();
    }

    wait_until(|| logins.lock().unwrap().contains(&2)).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    ws_client.close().await.unwrap();
    server.abort();
    let _ = server.await;
    let logins = logins.lock().unwrap().clone();

    assert_eq!(connections.load(Ordering::Acquire), 3);
    assert_eq!(AUTH_ATTEMPTS.load(Ordering::Acquire), 1);
    assert_eq!(
        logins,
        vec![0, 2],
        "backend={backend:?}, install_window={install_window}"
    );
}

async fn wait_until(condition: impl Fn() -> bool) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
}

async fn serve_logins(
    listener: tokio::net::TcpListener,
    connections: Arc<AtomicUsize>,
    logins: Arc<Mutex<Vec<usize>>>,
) {
    let mut peers = tokio::task::JoinSet::new();

    loop {
        let (socket, _) = listener.accept().await.unwrap();
        let connection = connections.fetch_add(1, Ordering::AcqRel);
        let logins = Arc::clone(&logins);

        peers.spawn(async move {
            let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
            while let Some(Ok(message)) = socket.next().await {
                match message {
                    Message::Text(text) if text == "ping" => {
                        let _ = socket.send(Message::Text("pong".into())).await;
                    }
                    Message::Text(text) => {
                        let payload: Value = serde_json::from_str(&text).unwrap();
                        if payload["op"] == "login" {
                            logins.lock().unwrap().push(connection);
                            // No replacement receives an ACK: stale responses cannot explain either race
                            if connection == 0 {
                                let response = json!({"event": "login", "code": "0", "msg": "", "connId": "initial"});
                                let _ = socket.send(Message::Text(response.to_string().into())).await;
                            }
                        } else if payload["op"] == "subscribe" {
                            for arg in payload["args"].as_array().unwrap() {
                                let response = json!({"event": "subscribe", "arg": arg, "connId": "initial"});
                                let _ = socket.send(Message::Text(response.to_string().into())).await;
                            }
                        }
                    }
                    Message::Ping(payload) => {
                        let _ = socket.send(Message::Pong(payload)).await;
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
        });
    }
}

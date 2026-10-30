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
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use futures_util::{SinkExt, StreamExt};
use nautilus_live::{SocketControl, SocketReconnectRegistry, SocketReconnectRequestOutcome};
use nautilus_model::identifiers::ClientId;
use nautilus_network::websocket::TransportBackend;
use nautilus_okx::{common::enums::OKXInstrumentType, websocket::client::OKXWebSocketClient};
use rstest::rstest;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

#[rstest]
#[case::tungstenite_acknowledged(TransportBackend::Tungstenite, true)]
#[case::tungstenite_pending(TransportBackend::Tungstenite, false)]
#[case::sockudo_acknowledged(TransportBackend::Sockudo, true)]
#[case::sockudo_pending(TransportBackend::Sockudo, false)]
#[tokio::test]
async fn reconnects_authenticate_and_resubscribe(
    #[case] backend: TransportBackend,
    #[case] acknowledge_first: bool,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let connections = Arc::new(AtomicUsize::new(0));
    let (operations, mut received) = tokio::sync::mpsc::unbounded_channel();
    let (shutdown, stopping) = tokio::sync::oneshot::channel();

    let server = tokio::spawn(serve_logins(
        listener,
        Arc::clone(&connections),
        operations,
        acknowledge_first,
        stopping,
    ));
    let registry = SocketReconnectRegistry::default();
    let client_id = ClientId::from("RECONNECT-LOGIN");
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
    expect_operation(&mut received, 0, "login").await;
    ws_client
        .subscribe_orders(OKXInstrumentType::Spot)
        .await
        .unwrap();
    expect_operation(&mut received, 0, "subscribe").await;
    let reconnect = registry.handle(client_id, Ustr::from("private")).unwrap();
    assert_eq!(
        reconnect.request_reconnect(),
        SocketReconnectRequestOutcome::Accepted
    );
    expect_operation(&mut received, 1, "login").await;
    if acknowledge_first {
        expect_operation(&mut received, 1, "subscribe").await;
    }

    assert_eq!(
        reconnect.request_reconnect(),
        SocketReconnectRequestOutcome::Accepted
    );
    expect_operation(&mut received, 2, "login").await;
    expect_operation(&mut received, 2, "subscribe").await;
    ws_client.close().await.unwrap();
    shutdown.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(connections.load(Ordering::Acquire), 3);
    assert_eq!(
        received.try_recv().unwrap_err(),
        tokio::sync::mpsc::error::TryRecvError::Disconnected
    );
}

async fn expect_operation(
    received: &mut tokio::sync::mpsc::UnboundedReceiver<(usize, Value)>,
    connection: usize,
    operation: &str,
) {
    let (observed_connection, payload) =
        tokio::time::timeout(Duration::from_secs(5), received.recv())
            .await
            .unwrap_or_else(|e| {
                panic!("Timed out waiting for {operation} on connection {connection}: {e}")
            })
            .expect("mock server stopped before receiving operation");

    assert_eq!(observed_connection, connection, "operation={payload}");
    assert_eq!(payload["op"], operation, "connection={connection}");
}

async fn serve_logins(
    listener: tokio::net::TcpListener,
    connections: Arc<AtomicUsize>,
    operations: tokio::sync::mpsc::UnboundedSender<(usize, Value)>,
    acknowledge_first: bool,
    mut stopping: tokio::sync::oneshot::Receiver<()>,
) {
    let mut peers = tokio::task::JoinSet::new();

    loop {
        let accepted = tokio::select! {
            biased;
            _ = &mut stopping => break,
            accepted = listener.accept() => accepted,
        };
        let (socket, _) = accepted.unwrap();
        let connection = connections.fetch_add(1, Ordering::AcqRel);
        let operations = operations.clone();

        peers.spawn(async move {
            let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
            while let Some(Ok(message)) = socket.next().await {
                match message {
                    Message::Text(text) if text == "ping" => {
                        let _ = socket.send(Message::Text("pong".into())).await;
                    }
                    Message::Text(text) => {
                        let payload: Value = serde_json::from_str(&text).unwrap();
                        if payload["op"] == "login" && (connection != 1 || acknowledge_first) {
                            let response = json!({"event": "login", "code": "0", "msg": "", "connId": format!("connection-{connection}")});
                            socket.send(Message::Text(response.to_string().into())).await.unwrap();
                        } else if payload["op"] == "subscribe" {
                            for arg in payload["args"].as_array().unwrap() {
                                let response = json!({"event": "subscribe", "arg": arg, "connId": format!("connection-{connection}")});
                                socket.send(Message::Text(response.to_string().into())).await.unwrap();
                            }
                        }

                        operations.send((connection, payload)).unwrap();
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

    peers.shutdown().await;
}

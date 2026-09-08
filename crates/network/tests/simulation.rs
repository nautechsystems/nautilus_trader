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

//! WebSocket lifecycle checks on the deterministic transport path.

#![cfg(all(feature = "simulation", madsim))]

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use nautilus_network::{
    RECONNECTED,
    backoff::ExponentialBackoff,
    dst::net::TcpListener,
    websocket::{
        WebSocketClient, WebSocketConfig, config::TransportBackend, types::channel_message_handler,
    },
};
use tokio_tungstenite::{accept_async, tungstenite::Message};

#[cfg(feature = "transport-sockudo")]
#[madsim::test]
async fn sockudo_is_rejected_before_transport_start() {
    let (handler, _messages) = channel_message_handler();
    let config = WebSocketConfig::builder()
        .url("ws://127.0.0.1:18081/feed".into())
        .backend(TransportBackend::Sockudo)
        .build()
        .unwrap();
    let error = WebSocketClient::builder()
        .config(config)
        .message_handler(handler)
        .connect()
        .await
        .unwrap_err();
    assert!(
        matches!(error, nautilus_network::TransportError::Other(ref message)
        if message == "Sockudo timers are unsupported under simulation; select Tungstenite")
    );
}

#[madsim::test]
async fn tungstenite_reconnect_preserves_payloads() {
    madsim::time::timeout(Duration::from_secs(10), async {
        let listener = TcpListener::bind("127.0.0.1:18081").await.unwrap();

        let peer = madsim::task::spawn(async move {
            let mut requests = Vec::new();

            for generation in 1..=2 {
                let (stream, _) = listener.accept().await.unwrap();
                let mut socket = accept_async(stream).await.unwrap();
                requests.push(socket.next().await.unwrap().unwrap());
                socket
                    .send(Message::text(format!("state-{generation}")))
                    .await
                    .unwrap();
                if generation == 1 {
                    socket.close(None).await.unwrap();
                } else {
                    assert!(matches!(
                        socket.next().await,
                        Some(Ok(Message::Close(None)))
                    ));
                }
            }
            requests
        });
        let (handler, mut messages) = channel_message_handler();
        let config = WebSocketConfig::builder()
            .url("ws://127.0.0.1:18081/feed".into())
            .backend(TransportBackend::Tungstenite)
            .build()
            .unwrap();
        let client = WebSocketClient::builder()
            .config(config)
            .message_handler(handler)
            .connect()
            .await
            .unwrap();
        client.send_text("subscribe".into(), None).await.unwrap();
        let first = messages.recv().await.unwrap();
        let reconnected = messages.recv().await.unwrap();
        client.send_text("subscribe".into(), None).await.unwrap();
        let second = messages.recv().await.unwrap();
        client.disconnect().await;
        let requests = peer.await.unwrap();

        assert_eq!(first, Message::text("state-1"));
        assert_eq!(reconnected, Message::text(RECONNECTED));
        assert_eq!(second, Message::text("state-2"));
        assert_eq!(
            requests,
            vec![Message::text("subscribe"), Message::text("subscribe")]
        );
        assert!(client.is_disconnected());
    })
    .await
    .unwrap();
}

#[rstest::rstest]
#[case::tls("wss://127.0.0.1:18081/feed", None)]
#[case::proxy("ws://127.0.0.1:18081/feed", Some("http://127.0.0.1:18082"))]
#[madsim::test]
async fn unsupported_endpoints_are_rejected(#[case] url: &str, #[case] proxy: Option<&str>) {
    let (handler, _messages) = channel_message_handler();
    let config = WebSocketConfig::builder()
        .url(url.into())
        .backend(TransportBackend::Tungstenite)
        .maybe_proxy_url(proxy.map(str::to_owned))
        .build()
        .unwrap();
    let error = WebSocketClient::builder()
        .config(config)
        .message_handler(handler)
        .connect()
        .await
        .unwrap_err();
    assert!(
        matches!(error, nautilus_network::TransportError::InvalidUrl(message)
        if message == "WebSocket simulation requires plaintext ws:// endpoints without a proxy")
    );
}

#[rstest::rstest]
fn jitter_restarts_with_runtime_seed() {
    let sample = || {
        let runtime = madsim::runtime::Runtime::with_seed_and_config(7, Default::default());
        runtime.block_on(async {
            let mut backoff = ExponentialBackoff::new(
                Duration::from_millis(100),
                Duration::from_secs(3),
                2.0,
                100,
                false,
            )
            .unwrap();
            (0..8).map(|_| backoff.next_duration()).collect::<Vec<_>>()
        })
    };
    assert_eq!(sample(), sample());
}

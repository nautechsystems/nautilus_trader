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

//! Inbound WebSocket size limits for both transport backends.

#![cfg(not(feature = "turmoil"))]
#![cfg(not(all(feature = "simulation", madsim)))]

use std::{net::SocketAddr, time::Duration};

use futures_util::{SinkExt, StreamExt};
use nautilus_network::{
    transport::{Message, TransportError},
    websocket::{TransportBackend, WebSocketClient, WebSocketConfig},
};
use rstest::rstest;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
};
use tokio_tungstenite::{accept_async, tungstenite};

const MESSAGE_PAYLOAD: &str = "0123456789abcdef";
const CAP_BYTES: usize = 8;

// Sockudo's small-frame parser skips the frame cap for payloads of 125 bytes or less
const FRAME_PAYLOAD_LEN: usize = 126;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InboundExpectation {
    MessageTooLarge,
    #[cfg(feature = "transport-sockudo")]
    FrameTooLarge,
    Text,
}

fn config(
    url: String,
    backend: TransportBackend,
    proxy_url: Option<String>,
    max_message_size_bytes: Option<usize>,
    max_frame_size_bytes: Option<usize>,
) -> WebSocketConfig {
    WebSocketConfig {
        url,
        headers: vec![],
        heartbeat_interval_secs: None,
        heartbeat_payload: None,
        connect_timeout_ms: Some(2_000),
        reconnect_delay_initial_ms: None,
        reconnect_delay_max_ms: None,
        reconnect_backoff_factor: None,
        reconnect_jitter_ms: None,
        reconnect_max_attempts: Some(0),
        heartbeat_timeout_secs: None,
        idle_timeout_ms: None,
        backend,
        proxy_url,
        max_message_size_bytes,
        max_frame_size_bytes,
    }
}

#[cfg(feature = "transport-sockudo")]
async fn spawn_ping_sender(payload: String) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(stream).await.unwrap();
        socket
            .send(tungstenite::Message::Ping(payload.into_bytes().into()))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_secs(5)).await;
    });

    addr
}

async fn spawn_sender(payload: String) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(stream).await.unwrap();
        socket
            .send(tungstenite::Message::Text(payload.into()))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_secs(5)).await;
    });

    addr
}

async fn spawn_connect_proxy(upstream: SocketAddr) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };

            tokio::spawn(async move {
                let _ = tunnel_connect(stream, upstream).await;
            });
        }
    });

    addr
}

async fn tunnel_connect(stream: TcpStream, upstream: SocketAddr) -> std::io::Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut request_line = String::new();
    reader.read_line(&mut request_line).await?;
    if !request_line.starts_with("CONNECT ") {
        write_half
            .write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n")
            .await?;
        return Ok(());
    }

    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).await?;
        if n == 0 || line == "\r\n" {
            break;
        }
    }

    write_half
        .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
        .await?;
    write_half.flush().await?;

    let mut upstream_stream = TcpStream::connect(upstream).await?;
    let (mut upstream_read, mut upstream_write) = upstream_stream.split();
    let mut client_read = reader.into_inner();

    let client_to_upstream = async {
        let mut buf = vec![0u8; 8192];

        loop {
            let n = client_read.read(&mut buf).await?;
            if n == 0 {
                break;
            }

            upstream_write.write_all(&buf[..n]).await?;
        }

        Ok::<_, std::io::Error>(())
    };

    let upstream_to_client = async {
        let mut buf = vec![0u8; 8192];

        loop {
            let n = upstream_read.read(&mut buf).await?;
            if n == 0 {
                break;
            }

            write_half.write_all(&buf[..n]).await?;
        }

        Ok::<_, std::io::Error>(())
    };

    tokio::select! {
        result = client_to_upstream => result?,
        result = upstream_to_client => result?,
    }
    Ok(())
}

async fn read_first(config: WebSocketConfig) -> Option<Result<Message, TransportError>> {
    let (mut reader, _client) = WebSocketClient::stream_builder()
        .config(config)
        .connect()
        .await
        .expect("websocket connect");
    tokio::time::timeout(Duration::from_secs(2), reader.next())
        .await
        .expect("inbound read timed out")
}

fn assert_inbound(
    message: &Option<Result<Message, TransportError>>,
    expected: InboundExpectation,
    payload: &str,
) {
    match expected {
        InboundExpectation::MessageTooLarge => assert!(
            matches!(message, Some(Err(TransportError::MessageTooLarge))),
            "expected MessageTooLarge, was {message:?}"
        ),
        #[cfg(feature = "transport-sockudo")]
        InboundExpectation::FrameTooLarge => assert!(
            matches!(message, Some(Err(TransportError::FrameTooLarge))),
            "expected FrameTooLarge, was {message:?}"
        ),
        InboundExpectation::Text => assert!(
            matches!(
                &message,
                Some(Ok(Message::Text(bytes))) if bytes.as_ref() == payload.as_bytes()
            ),
            "expected the payload, was {message:?}"
        ),
    }
}

#[rstest]
#[case::tungstenite_message(
    TransportBackend::Tungstenite,
    Some(CAP_BYTES),
    None,
    InboundExpectation::MessageTooLarge,
    MESSAGE_PAYLOAD.len()
)]
#[case::tungstenite_frame(
    TransportBackend::Tungstenite,
    None,
    Some(CAP_BYTES),
    InboundExpectation::MessageTooLarge,
    FRAME_PAYLOAD_LEN
)]
#[case::tungstenite_at_cap(
    TransportBackend::Tungstenite,
    Some(MESSAGE_PAYLOAD.len()),
    None,
    InboundExpectation::Text,
    MESSAGE_PAYLOAD.len()
)]
#[case::tungstenite_unset(
    TransportBackend::Tungstenite,
    None,
    None,
    InboundExpectation::Text,
    MESSAGE_PAYLOAD.len()
)]
#[cfg_attr(
    feature = "transport-sockudo",
    case::sockudo_message(
        TransportBackend::Sockudo,
        Some(CAP_BYTES),
        None,
        InboundExpectation::MessageTooLarge,
        MESSAGE_PAYLOAD.len()
    )
)]
#[cfg_attr(
    feature = "transport-sockudo",
    case::sockudo_frame(
        TransportBackend::Sockudo,
        None,
        Some(CAP_BYTES),
        InboundExpectation::FrameTooLarge,
        FRAME_PAYLOAD_LEN
    )
)]
#[cfg_attr(
    feature = "transport-sockudo",
    case::sockudo_at_cap(
        TransportBackend::Sockudo,
        Some(MESSAGE_PAYLOAD.len()),
        None,
        InboundExpectation::Text,
        MESSAGE_PAYLOAD.len()
    )
)]
#[cfg_attr(
    feature = "transport-sockudo",
    case::sockudo_unset(
        TransportBackend::Sockudo,
        None,
        None,
        InboundExpectation::Text,
        MESSAGE_PAYLOAD.len()
    )
)]
#[tokio::test]
async fn inbound_size_limits_apply_per_connection(
    #[case] backend: TransportBackend,
    #[case] max_message_size_bytes: Option<usize>,
    #[case] max_frame_size_bytes: Option<usize>,
    #[case] expected: InboundExpectation,
    #[case] payload_len: usize,
) {
    let payload = "x".repeat(payload_len);
    let upstream = spawn_sender(payload.clone()).await;
    let message = read_first(config(
        format!("ws://{upstream}"),
        backend,
        None,
        max_message_size_bytes,
        max_frame_size_bytes,
    ))
    .await;

    assert_inbound(&message, expected, &payload);
}

#[rstest]
#[case::message(Some(0), None, "max_message_size_bytes")]
#[case::frame(None, Some(0), "max_frame_size_bytes")]
#[tokio::test]
async fn stream_mode_rejects_zero_inbound_size_limit(
    #[case] max_message_size_bytes: Option<usize>,
    #[case] max_frame_size_bytes: Option<usize>,
    #[case] field: &str,
) {
    let error = match WebSocketClient::stream_builder()
        .config(config(
            "ws://127.0.0.1:9".to_string(),
            TransportBackend::Tungstenite,
            None,
            max_message_size_bytes,
            max_frame_size_bytes,
        ))
        .connect()
        .await
    {
        Err(e) => e,
        Ok(_) => panic!("zero size limit should be rejected before connect"),
    };

    let TransportError::Io(error) = error else {
        panic!("expected invalid input, was {error:?}");
    };

    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(
        error.to_string().contains(field),
        "expected {field} in {error}"
    );
}

#[cfg(feature = "transport-sockudo")]
#[rstest]
#[tokio::test]
async fn sockudo_message_cap_allows_oversized_control_ping() {
    let payload = "x".repeat(MESSAGE_PAYLOAD.len());
    let upstream = spawn_ping_sender(payload.clone()).await;
    let message = read_first(config(
        format!("ws://{upstream}"),
        TransportBackend::Sockudo,
        None,
        Some(CAP_BYTES),
        None,
    ))
    .await;

    assert!(
        matches!(
            &message,
            Some(Ok(Message::Ping(bytes))) if bytes.as_ref() == payload.as_bytes()
        ),
        "expected the ping, was {message:?}"
    );
}

#[rstest]
#[case::tungstenite(TransportBackend::Tungstenite)]
#[cfg_attr(
    feature = "transport-sockudo",
    case::sockudo(TransportBackend::Sockudo)
)]
#[tokio::test]
async fn proxied_inbound_message_cap_rejects_oversized_message(#[case] backend: TransportBackend) {
    let upstream = spawn_sender(MESSAGE_PAYLOAD.to_string()).await;
    let proxy = spawn_connect_proxy(upstream).await;
    let message = read_first(config(
        format!("ws://{upstream}"),
        backend,
        Some(format!("http://{proxy}")),
        Some(CAP_BYTES),
        None,
    ))
    .await;

    assert_inbound(
        &message,
        InboundExpectation::MessageTooLarge,
        MESSAGE_PAYLOAD,
    );
}

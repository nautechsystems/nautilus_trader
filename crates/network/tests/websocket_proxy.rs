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

//! End-to-end tests for the WebSocket client's HTTP `CONNECT` proxy path.
//!
//! Spins up a tiny in-process proxy and a tiny WebSocket echo server on
//! distinct localhost ports. The proxy parses a single `CONNECT` request,
//! returns `200`, then bidirectionally pipes bytes between the client and
//! the upstream server. This exercises the same code path the production
//! `WebSocketConfig.proxy_url` follows for plain `ws://` upstreams.

#![cfg(not(feature = "turmoil"))]
// Transport-layer I/O is not simulated under DST (see docs/concepts/dst.md
// "Transport-layer I/O is not simulated"); these proxy/integration tests rely
// on real localhost sockets and panic when madsim's time primitives are
// reached outside a runtime.
#![cfg(not(all(feature = "simulation", madsim)))]

use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use futures_util::{SinkExt, StreamExt};
use nautilus_network::{
    transport::Message,
    websocket::{TransportBackend, WebSocketClient, WebSocketConfig, types::MessageHandler},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::Mutex,
};
use tokio_tungstenite::{accept_async, tungstenite};

/// Captured state from the in-process CONNECT proxy: how many CONNECT
/// requests it served and the headers seen on the most recent one.
#[derive(Default)]
struct ProxyCapture {
    /// Total number of CONNECT requests successfully tunnelled.
    connect_count: AtomicUsize,
    /// Headers from the most recent CONNECT (one entry per non-empty line
    /// after the request line).
    last_headers: Mutex<Vec<String>>,
}

/// Spawn a tiny HTTP `CONNECT` proxy that tunnels arbitrarily many clients
/// to the supplied upstream address. Returns the proxy's bound address and a
/// shared capture handle so tests can assert on what the proxy observed.
async fn spawn_connect_proxy(upstream: SocketAddr) -> (SocketAddr, Arc<ProxyCapture>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let capture = Arc::new(ProxyCapture::default());
    let capture_loop = Arc::clone(&capture);

    tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(pair) => pair,
                Err(_) => break,
            };
            let cap = Arc::clone(&capture_loop);
            tokio::spawn(async move {
                if let Err(e) = handle_connect(stream, upstream, cap).await {
                    eprintln!("proxy hop error: {e}");
                }
            });
        }
    });

    (addr, capture)
}

async fn handle_connect(
    stream: TcpStream,
    upstream: SocketAddr,
    capture: Arc<ProxyCapture>,
) -> std::io::Result<()> {
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

    let mut headers = Vec::new();
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).await?;
        if n == 0 || line == "\r\n" {
            break;
        }
        headers.push(line.trim_end_matches("\r\n").to_string());
    }
    *capture.last_headers.lock().await = headers;
    capture.connect_count.fetch_add(1, Ordering::SeqCst);

    write_half
        .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
        .await?;
    write_half.flush().await?;

    let mut upstream_stream = TcpStream::connect(upstream).await?;
    let (mut up_read, mut up_write) = upstream_stream.split();
    let mut client_read = reader.into_inner();

    // Pipe both directions until either side closes
    let client_to_upstream = async {
        let mut buf = vec![0u8; 8192];
        loop {
            let n = client_read.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            up_write.write_all(&buf[..n]).await?;
        }
        Ok::<_, std::io::Error>(())
    };
    let upstream_to_client = async {
        let mut buf = vec![0u8; 8192];
        loop {
            let n = up_read.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            write_half.write_all(&buf[..n]).await?;
        }
        Ok::<_, std::io::Error>(())
    };

    tokio::select! {
        res = client_to_upstream => res?,
        res = upstream_to_client => res?,
    }
    Ok(())
}

/// Spawn a one-shot WebSocket echo server. Returns the bound address and
/// pushes received text frames into the supplied buffer.
async fn spawn_echo_server(received: Arc<Mutex<Vec<String>>>) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(stream).await.expect("ws handshake");
        while let Some(msg) = ws.next().await {
            match msg {
                Ok(tungstenite::Message::Text(t)) => {
                    received.lock().await.push(t.to_string());
                    if ws
                        .send(tungstenite::Message::Text(t.clone()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(tungstenite::Message::Close(_)) => break,
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });

    addr
}

#[tokio::test]
async fn websocket_client_routes_through_http_connect_proxy() {
    let received: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let echo_addr = spawn_echo_server(Arc::clone(&received)).await;
    let (proxy_addr, _capture) = spawn_connect_proxy(echo_addr).await;

    let target_url = format!("ws://{echo_addr}/");
    let proxy_url = format!("http://{proxy_addr}");

    let echoes: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let echoes_clone = Arc::clone(&echoes);
    let handler: MessageHandler = Arc::new(move |msg: Message| {
        if let Message::Text(b) = msg {
            let s = String::from_utf8_lossy(&b).to_string();
            let echoes = Arc::clone(&echoes_clone);
            tokio::spawn(async move {
                echoes.lock().await.push(s);
            });
        }
    });

    let config = WebSocketConfig {
        url: target_url,
        headers: vec![],
        heartbeat: None,
        heartbeat_msg: None,
        reconnect_timeout_ms: Some(5_000),
        reconnect_delay_initial_ms: Some(50),
        reconnect_delay_max_ms: Some(200),
        reconnect_backoff_factor: Some(1.5),
        reconnect_jitter_ms: Some(10),
        reconnect_max_attempts: Some(0),
        idle_timeout_ms: None,
        backend: TransportBackend::Tungstenite,
        proxy_url: Some(proxy_url),
    };

    let client = WebSocketClient::connect(config, Some(handler), None, None, vec![], None)
        .await
        .expect("connect through proxy");

    client
        .send_text("hello via proxy".to_string(), None)
        .await
        .unwrap();

    // Wait briefly for the round-trip.
    for _ in 0..40 {
        if !received.lock().await.is_empty() && !echoes.lock().await.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let received_msgs = received.lock().await.clone();
    let echoed_msgs = echoes.lock().await.clone();
    client.disconnect().await;

    assert_eq!(received_msgs, vec!["hello via proxy".to_string()]);
    assert_eq!(echoed_msgs, vec!["hello via proxy".to_string()]);
}

#[tokio::test]
async fn websocket_client_falls_back_to_direct_for_socks_proxy() {
    // SOCKS proxies are not yet supported on the WS path. To preserve REST
    // proxy configs that use SOCKS, the client should log a warning and fall
    // back to a direct connection rather than failing the handshake. We verify
    // the direct path works by spinning up an echo server and pointing the
    // target URL at it while supplying a SOCKS proxy_url.
    let received: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let echo_addr = spawn_echo_server(Arc::clone(&received)).await;

    let config = WebSocketConfig {
        url: format!("ws://{echo_addr}/"),
        headers: vec![],
        heartbeat: None,
        heartbeat_msg: None,
        reconnect_timeout_ms: Some(2_000),
        reconnect_delay_initial_ms: Some(10),
        reconnect_delay_max_ms: Some(50),
        reconnect_backoff_factor: Some(1.5),
        reconnect_jitter_ms: Some(10),
        reconnect_max_attempts: Some(0),
        idle_timeout_ms: None,
        backend: TransportBackend::Tungstenite,
        proxy_url: Some("socks5://127.0.0.1:1080".to_string()),
    };

    let handler: MessageHandler = Arc::new(|_| {});
    let client = WebSocketClient::connect(config, Some(handler), None, None, vec![], None)
        .await
        .expect("connect should succeed via direct fallback when SOCKS is requested");
    client.send_text("ping".to_string(), None).await.unwrap();

    for _ in 0..40 {
        if !received.lock().await.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let msgs = received.lock().await.clone();
    client.disconnect().await;
    assert_eq!(msgs, vec!["ping".to_string()]);
}

/// Spawn an echo server that drops the *first* incoming WebSocket
/// connection after the upgrade, then echoes for subsequent connects. Used
/// to drive a forced reconnect.
async fn spawn_one_drop_echo_server(received: Arc<Mutex<Vec<String>>>) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        // First client: complete the handshake then drop immediately
        if let Ok(mut ws) = accept_async(stream).await {
            let _ = ws.close(None).await;
        }

        // Subsequent clients: echo until close
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(pair) => pair,
                Err(_) => break,
            };
            let received = Arc::clone(&received);

            tokio::spawn(async move {
                let mut ws = accept_async(stream).await.expect("ws handshake");
                while let Some(msg) = ws.next().await {
                    match msg {
                        Ok(tungstenite::Message::Text(t)) => {
                            received.lock().await.push(t.to_string());
                            if ws
                                .send(tungstenite::Message::Text(t.clone()))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                        Ok(tungstenite::Message::Close(_)) => break,
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
            });
        }
    });

    addr
}

/// The client must send `Proxy-Authorization` when proxy_url embeds basic
/// auth credentials. Without this assertion, a regression that drops the
/// header would only surface against a real authenticated proxy.
#[tokio::test]
async fn websocket_client_emits_proxy_authorization_header() {
    let received: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let echo_addr = spawn_echo_server(Arc::clone(&received)).await;
    let (proxy_addr, capture) = spawn_connect_proxy(echo_addr).await;

    let proxy_url = format!("http://proxytest:fixture42@{proxy_addr}");

    let config = WebSocketConfig {
        url: format!("ws://{echo_addr}/"),
        headers: vec![],
        heartbeat: None,
        heartbeat_msg: None,
        reconnect_timeout_ms: Some(5_000),
        reconnect_delay_initial_ms: Some(50),
        reconnect_delay_max_ms: Some(200),
        reconnect_backoff_factor: Some(1.5),
        reconnect_jitter_ms: Some(10),
        reconnect_max_attempts: Some(0),
        idle_timeout_ms: None,
        backend: TransportBackend::Tungstenite,
        proxy_url: Some(proxy_url),
    };

    let handler: MessageHandler = Arc::new(|_| {});
    let client = WebSocketClient::connect(config, Some(handler), None, None, vec![], None)
        .await
        .expect("connect through authenticated proxy");
    client.disconnect().await;

    let headers = capture.last_headers.lock().await.clone();
    let auth = headers
        .iter()
        .find(|h| h.to_ascii_lowercase().starts_with("proxy-authorization:"))
        .expect("expected Proxy-Authorization header on CONNECT");
    // base64("proxytest:fixture42") == "cHJveHl0ZXN0OmZpeHR1cmU0Mg=="
    assert!(
        auth.contains("Basic cHJveHl0ZXN0OmZpeHR1cmU0Mg=="),
        "header was {auth:?}, full headers: {headers:?}"
    );
}

/// Reconnects must continue to use the configured proxy. The fixture's
/// first WS connection is dropped after the handshake, forcing the client
/// to reconnect; both connect attempts must be observed by the proxy.
#[tokio::test]
async fn websocket_client_reuses_proxy_url_on_reconnect() {
    let received: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let echo_addr = spawn_one_drop_echo_server(Arc::clone(&received)).await;
    let (proxy_addr, capture) = spawn_connect_proxy(echo_addr).await;

    let config = WebSocketConfig {
        url: format!("ws://{echo_addr}/"),
        headers: vec![],
        heartbeat: None,
        heartbeat_msg: None,
        reconnect_timeout_ms: Some(5_000),
        reconnect_delay_initial_ms: Some(20),
        reconnect_delay_max_ms: Some(100),
        reconnect_backoff_factor: Some(1.5),
        reconnect_jitter_ms: Some(5),
        reconnect_max_attempts: Some(5),
        idle_timeout_ms: None,
        backend: TransportBackend::Tungstenite,
        proxy_url: Some(format!("http://{proxy_addr}")),
    };

    let handler: MessageHandler = Arc::new(|_| {});
    let client = WebSocketClient::connect(config, Some(handler), None, None, vec![], None)
        .await
        .expect("initial connect through proxy");

    // Wait for the reconnect to land: the upstream drops the first session,
    // so the client should re-issue CONNECT through the proxy at least once.
    for _ in 0..100 {
        if capture.connect_count.load(Ordering::SeqCst) >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let connects = capture.connect_count.load(Ordering::SeqCst);
    client.disconnect().await;
    assert!(
        connects >= 2,
        "expected at least 2 CONNECT requests through proxy after reconnect, was {connects}"
    );
}

/// The Sockudo backend cannot tunnel through a proxy yet. The dispatcher
/// must reject `proxy_url` with a clear error rather than silently
/// connecting direct.
#[tokio::test]
async fn sockudo_backend_rejects_proxy_url() {
    let config = WebSocketConfig {
        url: "ws://127.0.0.1:1/".to_string(),
        headers: vec![],
        heartbeat: None,
        heartbeat_msg: None,
        reconnect_timeout_ms: Some(500),
        reconnect_delay_initial_ms: Some(10),
        reconnect_delay_max_ms: Some(50),
        reconnect_backoff_factor: Some(1.5),
        reconnect_jitter_ms: Some(10),
        reconnect_max_attempts: Some(0),
        idle_timeout_ms: None,
        backend: TransportBackend::Sockudo,
        proxy_url: Some("http://127.0.0.1:9999".to_string()),
    };

    let handler: MessageHandler = Arc::new(|_| {});
    let err = WebSocketClient::connect(config, Some(handler), None, None, vec![], None)
        .await
        .expect_err("Sockudo + proxy_url should error");
    let msg = err.to_string();
    assert!(
        msg.contains("Sockudo") || msg.contains("sockudo"),
        "unexpected error: {msg}"
    );
}

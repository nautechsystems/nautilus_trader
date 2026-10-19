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

use std::time::Duration;

use nautilus_kraken::{
    config::KrakenDataClientConfig,
    websocket::{
        futures::client::KrakenFuturesWebSocketClient, spot_v2::client::KrakenSpotWebSocketClient,
    },
};
use nautilus_network::websocket::TransportBackend;
use rstest::rstest;
use tokio_util::sync::CancellationToken;

#[path = "../../../network/tests/integration/common/client_logging.rs"]
mod client_logging;

use client_logging::{REQUEST_TARGET, capture_logs, messages, start_server};

#[rstest]
#[case(TransportBackend::Tungstenite)]
#[case(TransportBackend::Sockudo)]
#[tokio::test]
async fn test_futures_connection_logging(#[case] backend: TransportBackend) {
    let _capture = capture_logs().await;
    let server = start_server(1, None).await;
    let mut client = KrakenFuturesWebSocketClient::with_credentials(
        server.url.clone(),
        30,
        None,
        None,
        backend,
        None,
    );
    let debug = format!("{client:?}");
    tokio::time::timeout(Duration::from_secs(5), async {
        client.connect().await.unwrap();
    })
    .await
    .unwrap();

    let requests = tokio::time::timeout(Duration::from_secs(5), server.requests)
        .await
        .unwrap()
        .unwrap();
    let logs = messages();
    client.disconnect().await.unwrap();
    server.task.abort();
    assert_eq!(requests, vec![REQUEST_TARGET.to_string(); 1]);
    assert!(debug.contains(&format!("url: {:?}", server.origin)));
    assert!(!debug.contains("URL_SECRET"));
    assert!(
        !logs
            .iter()
            .any(|(_, message)| message.contains("URL_SECRET")),
        "logs: {logs:?}"
    );
    assert!(
        logs.iter()
            .any(|(level, message)| *level == log::Level::Debug
                && message.contains(&server.origin)
                && message.contains("Connecting"))
    );
}

#[rstest]
#[case(TransportBackend::Tungstenite)]
#[case(TransportBackend::Sockudo)]
#[tokio::test]
async fn test_spot_connection_logging(#[case] backend: TransportBackend) {
    let _capture = capture_logs().await;
    let server = start_server(1, None).await;

    let mut client = KrakenSpotWebSocketClient::new(
        KrakenDataClientConfig {
            ws_public_url: Some(server.url.clone()),
            transport_backend: backend,
            ..Default::default()
        },
        CancellationToken::new(),
        None,
    );

    let debug = format!("{client:?}");
    tokio::time::timeout(Duration::from_secs(5), async {
        client.connect().await.unwrap();
    })
    .await
    .unwrap();

    let requests = tokio::time::timeout(Duration::from_secs(5), server.requests)
        .await
        .unwrap()
        .unwrap();
    let logs = messages();
    client.disconnect().await.unwrap();
    server.task.abort();
    assert_eq!(requests, vec![REQUEST_TARGET.to_string(); 1]);
    assert!(debug.contains(&format!("url: {:?}", server.origin)));
    assert!(!debug.contains("URL_SECRET"));
    assert!(
        !logs
            .iter()
            .any(|(_, message)| message.contains("URL_SECRET")),
        "logs: {logs:?}"
    );
    assert!(
        logs.iter()
            .any(|(level, message)| *level == log::Level::Debug
                && message.contains(&server.origin)
                && message.contains("Connecting"))
    );
}

#[tokio::test]
async fn test_futures_http_auth_logging() {
    use nautilus_kraken::{
        common::enums::KrakenEnvironment, http::futures::client::KrakenFuturesRawHttpClient,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let _capture = capture_logs().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            stream.read_exact(&mut byte).await.unwrap();
            request.push(byte[0]);
        }

        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
            .await
            .unwrap();
        String::from_utf8(request).unwrap()
    });

    let url = format!("http://URL_SECRET_USER:URL_SECRET_PASSWORD@{addr}/URL_SECRET_PATH");
    let client = KrakenFuturesRawHttpClient::with_credentials(
        "fake-api-key".to_string(),
        "c2VjcmV0".to_string(),
        KrakenEnvironment::Demo,
        Some(url),
        2,
        Some(0),
        None,
        None,
        None,
        10,
    )
    .unwrap();
    let debug = format!("{client:?}");
    let result = tokio::time::timeout(Duration::from_secs(5), client.get_accounts())
        .await
        .unwrap();
    let request = task.await.unwrap();
    let logs = messages();
    assert!(result.is_err());
    assert!(request.starts_with("GET /URL_SECRET_PATH/derivatives/api/v3/accounts HTTP/1.1\r\n"));
    assert!(debug.contains(&format!("base_url: \"http://{addr}\"")));
    assert!(!debug.contains("URL_SECRET"));
    assert!(
        logs.iter()
            .any(|(level, message)| *level == log::Level::Debug
                && message.starts_with(
                    "Kraken Futures auth: endpoint=/derivatives/api/v3/accounts, nonce="
                )
                && message.ends_with(&format!(", base_url=http://{addr}")))
    );
    assert!(
        !logs
            .iter()
            .any(|(_, message)| message.contains("URL_SECRET")),
        "logs: {logs:?}"
    );
}

#[tokio::test]
async fn test_futures_http_transport_error_retains_origin() {
    use nautilus_kraken::{
        common::enums::KrakenEnvironment, http::futures::client::KrakenFuturesRawHttpClient,
    };
    let _capture = capture_logs().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let url = format!("http://URL_SECRET_USER:URL_SECRET_PASSWORD@{addr}/URL_SECRET_PATH");
    let client = KrakenFuturesRawHttpClient::with_credentials(
        "fake-api-key".to_string(),
        "c2VjcmV0".to_string(),
        KrakenEnvironment::Demo,
        Some(url),
        1,
        Some(0),
        None,
        None,
        None,
        10,
    )
    .unwrap();
    let error = client.get_accounts().await.unwrap_err();
    assert!(error.to_string().contains(&format!("http://{addr}")));
    assert!(!format!("{error:?} {error}").contains("URL_SECRET"));
    let order_error = client
        .cancel_order(Some("public-order-id".to_string()), None)
        .await
        .unwrap_err();
    assert!(order_error.to_string().contains(&format!("http://{addr}")));
    assert!(!format!("{order_error:?} {order_error}").contains("URL_SECRET"));
    assert!(
        !messages()
            .iter()
            .any(|(_, message)| message.contains("URL_SECRET"))
    );
}

#[tokio::test]
async fn test_spot_http_logging_and_wire_url() {
    use nautilus_kraken::{
        common::enums::KrakenEnvironment, http::spot::client::KrakenSpotRawHttpClient,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let _capture = capture_logs().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            stream.read_exact(&mut byte).await.unwrap();
            request.push(byte[0]);
        }

        let body = r#"{"error":[],"result":{"unixtime":1700000001,"rfc1123":"Tue, 14 Nov 2023 22:13:21 GMT"}}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).await.unwrap();
        String::from_utf8(request).unwrap()
    });

    let url = format!("http://URL_SECRET_USER:URL_SECRET_PASSWORD@{addr}/URL_SECRET_PATH");
    let client = KrakenSpotRawHttpClient::new(
        KrakenEnvironment::Live,
        Some(url.clone()),
        2,
        Some(0),
        None,
        None,
        None,
        10,
    )
    .unwrap();
    let debug = format!("{client:?}");
    let result = tokio::time::timeout(Duration::from_secs(5), client.get_server_time())
        .await
        .unwrap()
        .unwrap();
    let request = task.await.unwrap();
    assert_eq!(client.base_url(), url);
    assert_eq!(result.unixtime, 1700000001);
    assert_eq!(result.rfc1123, "Tue, 14 Nov 2023 22:13:21 GMT");
    assert!(request.starts_with("GET /URL_SECRET_PATH/0/public/Time HTTP/1.1\r\n"));
    assert!(debug.contains(&format!("base_url: \"http://{addr}\"")));
    assert!(!debug.contains("URL_SECRET"));
    assert!(
        !messages()
            .iter()
            .any(|(_, message)| message.contains("URL_SECRET"))
    );
}

#[tokio::test]
async fn test_spot_http_transport_error_retains_origin() {
    use nautilus_kraken::{
        common::enums::KrakenEnvironment, http::spot::client::KrakenSpotRawHttpClient,
    };
    let _capture = capture_logs().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let url = format!("http://URL_SECRET_USER:URL_SECRET_PASSWORD@{addr}/URL_SECRET_PATH");
    let client = KrakenSpotRawHttpClient::with_credentials(
        "fake-api-key".to_string(),
        "c2VjcmV0".to_string(),
        KrakenEnvironment::Live,
        Some(url),
        1,
        Some(0),
        None,
        None,
        None,
        10,
    )
    .unwrap();
    let error = client.get_balance().await.unwrap_err();
    assert!(error.to_string().contains(&format!("http://{addr}")));
    assert!(!format!("{error:?} {error}").contains("URL_SECRET"));
    assert!(
        !messages()
            .iter()
            .any(|(_, message)| message.contains("URL_SECRET"))
    );
}

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

use nautilus_derive::{
    common::enums::DeriveEnvironment,
    websocket::client::{DeriveWebSocketClient, DeriveWsCredentials},
};
use nautilus_network::websocket::TransportBackend;
use rstest::rstest;

#[path = "../../../network/tests/integration/common/client_logging.rs"]
mod client_logging;

use client_logging::{REQUEST_TARGET, capture_logs, messages, start_server};

#[rstest]
#[case(TransportBackend::Tungstenite)]
#[case(TransportBackend::Sockudo)]
#[tokio::test]
async fn test_derive_connection_logging(#[case] backend: TransportBackend) {
    let _capture = capture_logs().await;
    let server = start_server(1, None).await;

    let mut client = DeriveWebSocketClient::new(
        Some(server.url.clone()),
        DeriveEnvironment::Testnet,
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
                && message.contains("connected"))
    );
}

#[tokio::test]
async fn test_login_error_logging_preserves_code() {
    let _capture = capture_logs().await;
    let server = start_server(1, None).await;
    let credentials = DeriveWsCredentials::new(
        "0x000000000000000000000000000000000000aaaa",
        "0x2ae8be44db8a590d20bffbe3b6872df9b569147d3bf6801a35a28281a4816bbd",
    )
    .unwrap();
    let mut client = DeriveWebSocketClient::with_credentials(
        Some(server.url.clone()),
        DeriveEnvironment::Testnet,
        TransportBackend::Tungstenite,
        None,
        credentials,
        None,
        None,
    );
    let result = tokio::time::timeout(Duration::from_secs(5), client.connect())
        .await
        .unwrap();
    let error = result.unwrap_err();
    let requests = tokio::time::timeout(Duration::from_secs(5), server.requests)
        .await
        .unwrap()
        .unwrap();
    let logs = messages();
    server.task.abort();
    assert_eq!(requests, vec![REQUEST_TARGET.to_string()]);
    assert!(error.to_string().contains("URL_SECRET_PROVIDER_MESSAGE"));
    assert!(logs.contains(&(
        log::Level::Warn,
        "Derive WebSocket login failed; tearing down transport: JSON-RPC error -32000".to_string()
    )));
    assert!(
        !logs
            .iter()
            .any(|(_, message)| message.contains("URL_SECRET")),
        "logs: {logs:?}"
    );
}

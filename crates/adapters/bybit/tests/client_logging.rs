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

use nautilus_bybit::{
    common::enums::{BybitEnvironment, BybitProductType},
    websocket::{client::BybitWebSocketClient, error::BybitWsError},
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
async fn test_bybit_connection_logging(#[case] backend: TransportBackend) {
    let _capture = capture_logs().await;
    let server = start_server(1, None).await;
    let mut client = BybitWebSocketClient::new_public_with(
        BybitProductType::Linear,
        BybitEnvironment::Testnet,
        Some(server.url.clone()),
        30,
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
    client.close().await.unwrap();
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
}

#[tokio::test]
async fn test_connection_error_retains_origin_and_status() {
    let _capture = capture_logs().await;
    let server = start_server(1, Some(401)).await;
    let mut client = BybitWebSocketClient::new_public_with(
        BybitProductType::Linear,
        BybitEnvironment::Testnet,
        Some(server.url.clone()),
        30,
        TransportBackend::Tungstenite,
        None,
    );
    let error = tokio::time::timeout(Duration::from_secs(5), client.connect())
        .await
        .unwrap()
        .unwrap_err();

    let BybitWsError::Transport(message) = error else {
        panic!("expected transport rejection")
    };

    let requests = tokio::time::timeout(Duration::from_secs(5), server.requests)
        .await
        .unwrap()
        .unwrap();
    server.task.abort();
    assert_eq!(requests, vec![REQUEST_TARGET.to_string()]);
    assert_eq!(
        message,
        format!(
            "WebSocket connection failed: {}: WebSocket upgrade rejected with status 401",
            server.origin
        )
    );
    assert!(
        !messages()
            .iter()
            .any(|(_, message)| message.contains("URL_SECRET"))
    );
}

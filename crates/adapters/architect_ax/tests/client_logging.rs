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

use nautilus_architect_ax::websocket::{
    data::{AxMdWebSocketClient, AxWsClientError},
    orders::{AxOrdersWebSocketClient, AxOrdersWsClientError},
};
use nautilus_model::identifiers::{AccountId, TraderId};
use nautilus_network::websocket::TransportBackend;
use rstest::rstest;

#[path = "../../../network/tests/integration/common/client_logging.rs"]
mod client_logging;

use client_logging::{REQUEST_TARGET, capture_logs, messages, start_server};

#[rstest]
#[case(TransportBackend::Tungstenite)]
#[case(TransportBackend::Sockudo)]
#[tokio::test]
async fn test_data_connection_logging(#[case] backend: TransportBackend) {
    let _capture = capture_logs().await;
    let server = start_server(1, None).await;
    let mut client =
        AxMdWebSocketClient::new(server.url.clone(), "test-auth-token", 30, backend, None);
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

#[rstest]
#[case(TransportBackend::Tungstenite)]
#[case(TransportBackend::Sockudo)]
#[tokio::test]
async fn test_orders_connection_logging(#[case] backend: TransportBackend) {
    let _capture = capture_logs().await;
    let server = start_server(1, None).await;

    let mut client = AxOrdersWebSocketClient::new(
        server.url.clone(),
        AccountId::from("AX-001"),
        TraderId::from("TRADER-001"),
        30,
        backend,
        None,
    );
    let debug = format!("{client:?}");
    tokio::time::timeout(Duration::from_secs(5), async {
        client.connect("test-auth-token").await.unwrap();
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

#[rstest]
#[case(false)]
#[case(true)]
#[tokio::test]
async fn test_connection_error_retains_origin_and_status(#[case] orders: bool) {
    let _capture = capture_logs().await;
    let server = start_server(1, Some(401)).await;

    let message = if orders {
        let mut client = AxOrdersWebSocketClient::new(
            server.url.clone(),
            AccountId::from("AX-001"),
            TraderId::from("TRADER-001"),
            30,
            TransportBackend::Tungstenite,
            None,
        );
        let error = client.connect("test-auth-token").await.unwrap_err();

        let AxOrdersWsClientError::Transport(value) = error else {
            panic!("expected transport rejection")
        };

        value
    } else {
        let mut client = AxMdWebSocketClient::new(
            server.url.clone(),
            "test-auth-token",
            30,
            TransportBackend::Tungstenite,
            None,
        );
        let error = client.connect().await.unwrap_err();

        let AxWsClientError::Transport(value) = error else {
            panic!("expected transport rejection")
        };

        value
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

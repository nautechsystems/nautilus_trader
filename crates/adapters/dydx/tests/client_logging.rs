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

use std::{sync::Arc, time::Duration};

use nautilus_dydx::{
    common::instrument_cache::InstrumentCache, grpc::client::DydxGrpcClient,
    websocket::client::DydxWebSocketClient,
};
use nautilus_model::identifiers::InstrumentId;
use nautilus_network::websocket::TransportBackend;
use rstest::rstest;

#[path = "../../../network/tests/integration/common/client_logging.rs"]
mod client_logging;

use client_logging::{REQUEST_TARGET, capture_logs, messages, start_server};

#[rstest]
#[case(TransportBackend::Tungstenite)]
#[case(TransportBackend::Sockudo)]
#[tokio::test]
async fn test_dydx_connection_logging(#[case] backend: TransportBackend) {
    let _capture = capture_logs().await;
    let server = start_server(2, None).await;
    let mut client = DydxWebSocketClient::new_public_with_cache_and_pool(
        server.url.clone(),
        Arc::new(InstrumentCache::new()),
        None,
        backend,
        None,
        2,
        1,
    );
    let debug = format!("{client:?}");
    tokio::time::timeout(Duration::from_secs(5), async {
        client.connect().await.unwrap();
        client
            .subscribe_trades(InstrumentId::from("BTC-USD-PERP.DYDX"))
            .await
            .unwrap();
        client
            .subscribe_trades(InstrumentId::from("ETH-USD-PERP.DYDX"))
            .await
            .unwrap();
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
    assert_eq!(requests, vec![REQUEST_TARGET.to_string(); 2]);
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
                && message.contains("Connected"))
    );
    assert!(
        logs.iter()
            .any(|(level, message)| *level == log::Level::Debug
                && message.starts_with("dYdX pool slot 1 connected: url=")
                && message.contains(&server.origin))
    );
}

#[tokio::test]
async fn test_grpc_failure_logging_retains_origin() {
    let _capture = capture_logs().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let url = format!("http://{addr}/URL_SECRET_PATH?token=URL_SECRET_QUERY");
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        DydxGrpcClient::new_with_fallback(&[url]),
    )
    .await
    .unwrap();
    let logs = messages();
    assert!(result.is_err());
    assert!(
        !logs
            .iter()
            .any(|(_, message)| message.contains("URL_SECRET"))
    );
    assert_eq!(
        messages()
            .into_iter()
            .filter(|(_, message)| {
                message.starts_with("Attempting to connect to gRPC node:")
                    || message.starts_with("Failed to connect to gRPC node ")
            })
            .collect::<Vec<_>>(),
        vec![
            (
                log::Level::Debug,
                format!("Attempting to connect to gRPC node: http://{addr} (attempt 1/1)")
            ),
            (
                log::Level::Warn,
                format!("Failed to connect to gRPC node http://{addr}")
            )
        ]
    );
}

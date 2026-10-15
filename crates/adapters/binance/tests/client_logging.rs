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

use nautilus_binance::spot::websocket::streams::{
    BinanceSpotWebSocketClient, subscription::MAX_STREAMS_PER_CONNECTION,
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
async fn test_binance_connection_logging(#[case] backend: TransportBackend) {
    let _capture = capture_logs().await;
    let server = start_server(2, None).await;
    let mut client =
        BinanceSpotWebSocketClient::new(Some(server.url.clone()), None, None, None, backend)
            .unwrap();
    let debug = format!("{client:?}");
    tokio::time::timeout(Duration::from_secs(5), async {
        client.connect().await.unwrap();
        client
            .subscribe(
                (0..=MAX_STREAMS_PER_CONNECTION)
                    .map(|i| format!("symbol{i}@trade"))
                    .collect(),
            )
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
    client.close().await.unwrap();
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
                && message.contains("connected"))
    );
}

#[rstest]
#[case(TransportBackend::Tungstenite)]
#[case(TransportBackend::Sockudo)]
#[tokio::test]
async fn test_public_json_connection_logging(#[case] backend: TransportBackend) {
    use nautilus_binance::spot::websocket::public_json::client::{
        BinanceSpotPublicJsonWebSocketClient, MAX_STREAMS_PER_CONNECTION,
    };
    let _capture = capture_logs().await;
    let server = start_server(2, None).await;
    let url = format!("{}/stream", server.url.split('?').next().unwrap());
    let mut client = BinanceSpotPublicJsonWebSocketClient::new(Some(url), None, backend);
    let debug = format!("{client:?}");
    tokio::time::timeout(Duration::from_secs(5), async {
        client.connect().await.unwrap();
        client
            .subscribe(
                (0..=MAX_STREAMS_PER_CONNECTION)
                    .map(|i| format!("symbol{i}@trade"))
                    .collect(),
            )
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
    client.close().await.unwrap();
    server.task.abort();
    assert_eq!(requests, vec!["/URL_SECRET_PATH/stream".to_string(); 2]);
    assert!(debug.contains(&format!("url: {:?}", server.origin)));
    assert!(!debug.contains("URL_SECRET"));
    assert!(logs.contains(&(
        log::Level::Debug,
        format!(
            "Connected to Binance Spot public JSON stream pool: url={}",
            server.origin
        )
    )));
    assert!(logs.contains(&(
        log::Level::Debug,
        format!("Spot JSON pool slot 1 connected: url={}", server.origin)
    )));
    assert!(
        !logs
            .iter()
            .any(|(_, message)| message.contains("URL_SECRET")),
        "logs: {logs:?}"
    );
}

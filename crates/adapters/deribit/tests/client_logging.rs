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

use nautilus_deribit::{
    common::enums::DeribitEnvironment, websocket::client::DeribitWebSocketClient,
};
#[path = "../../../network/tests/integration/common/client_logging.rs"]
mod client_logging;
use client_logging::{REQUEST_TARGET, capture_logs, messages, start_server};

#[tokio::test]
async fn test_deribit_connection_logging() {
    let _capture = capture_logs().await;
    let server = start_server(1, None).await;
    let mut client = DeribitWebSocketClient::new_unauthenticated(
        Some(server.url.clone()),
        30,
        DeribitEnvironment::Testnet,
    )
    .unwrap();
    let debug = format!("{client:?}");
    tokio::time::timeout(Duration::from_secs(5), client.connect())
        .await
        .unwrap()
        .unwrap();
    let requests = tokio::time::timeout(Duration::from_secs(5), server.requests)
        .await
        .unwrap()
        .unwrap();
    let logs = messages();
    client.close().await.unwrap();
    server.task.abort();
    assert_eq!(requests, vec![REQUEST_TARGET.to_string()]);
    assert!(debug.contains(&format!("url: {:?}", server.origin)));
    assert!(!debug.contains("URL_SECRET"));
    assert!(logs.contains(&(
        log::Level::Debug,
        format!("Connecting to WebSocket: {}", server.origin)
    )));
    assert!(
        !logs
            .iter()
            .any(|(_, message)| message.contains("URL_SECRET")),
        "logs: {logs:?}"
    );
}

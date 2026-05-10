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

//! Shared test infrastructure for Betfair integration tests.

use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{
    Router,
    body::Bytes,
    extract::State,
    response::IntoResponse,
    routing::{get, post},
};
use nautilus_betfair::{
    common::credential::BetfairCredential, http::client::BetfairHttpClient,
    stream::config::BetfairStreamConfig,
};
use nautilus_common::testing::wait_until_async;
use nautilus_network::http::HttpClient;
use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
};

pub fn data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data")
}

pub fn load_fixture(path: &str) -> String {
    std::fs::read_to_string(data_path().join(path))
        .unwrap_or_else(|_| panic!("failed to read {path}"))
}

pub fn test_credential() -> BetfairCredential {
    BetfairCredential::new(
        "testuser".to_string(),
        "testpass".to_string(),
        "test-app-key".to_string(),
    )
}

pub fn plain_stream_config(port: u16) -> BetfairStreamConfig {
    BetfairStreamConfig {
        host: "127.0.0.1".to_string(),
        port,
        heartbeat_ms: 5_000,
        idle_timeout_ms: 60_000,
        reconnect_delay_initial_ms: 200,
        reconnect_delay_max_ms: 1_000,
        use_tls: false,
    }
}

#[derive(Clone, Default)]
pub struct MockState {
    pub login_count: Arc<AtomicUsize>,
}

async fn handle_login(State(state): State<MockState>) -> impl IntoResponse {
    state.login_count.fetch_add(1, Ordering::Relaxed);
    let body = load_fixture("rest/login_success.json");
    (
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        body,
    )
}

async fn handle_navigation() -> impl IntoResponse {
    let body = load_fixture("rest/navigation_list_navigation.json");
    (
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        body,
    )
}

async fn handle_betting(body: Bytes) -> impl IntoResponse {
    let request: Value = serde_json::from_slice(&body).unwrap_or_default();
    let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let id = request.get("id").and_then(|i| i.as_u64()).unwrap_or(0);

    let result = match method {
        "SportsAPING/v1.0/listMarketCatalogue" => {
            let fixture = load_fixture("rest/betting_list_market_catalogue.json");
            serde_json::from_str::<Value>(&fixture).unwrap()
        }
        "SportsAPING/v1.0/placeOrders" => {
            let fixture = load_fixture("rest/betting_place_order_success.json");
            let v: Value = serde_json::from_str(&fixture).unwrap();
            v["result"].clone()
        }
        _ => serde_json::json!(null),
    };

    let response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    });
    axum::Json(response)
}

async fn handle_accounts(body: Bytes) -> impl IntoResponse {
    let request: Value = serde_json::from_slice(&body).unwrap_or_default();
    let id = request.get("id").and_then(|i| i.as_u64()).unwrap_or(0);

    let fixture = load_fixture("rest/account_funds_no_exposure.json");
    let v: Value = serde_json::from_str(&fixture).unwrap();
    let result = v["result"].clone();

    let response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    });
    axum::Json(response)
}

pub async fn start_mock_http() -> (SocketAddr, MockState) {
    let state = MockState::default();

    let router = Router::new()
        .route("/login", post(handle_login))
        .route("/betting", post(handle_betting))
        .route("/accounts", post(handle_accounts))
        .route("/navigation", get(handle_navigation))
        .route("/health", get(|| async { "OK" }))
        .with_state(state.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    let health_client = HttpClient::new(
        std::collections::HashMap::new(),
        Vec::new(),
        Vec::new(),
        None,
        None,
        None,
    )
    .unwrap();

    wait_until_async(
        || {
            let url = format!("http://{addr}/health");
            let client = health_client.clone();
            async move { client.get(url, None, None, Some(1), None).await.is_ok() }
        },
        Duration::from_secs(5),
    )
    .await;

    (addr, state)
}

pub async fn start_mock_stream() -> (u16, TcpListener) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    (port, listener)
}

pub async fn accept_and_auth(
    listener: &TcpListener,
) -> (
    BufReader<tokio::net::tcp::OwnedReadHalf>,
    tokio::net::tcp::OwnedWriteHalf,
) {
    let (socket, _) = listener.accept().await.unwrap();
    let (read_half, mut write_half) = socket.into_split();
    let mut reader = BufReader::new(read_half);

    write_half
        .write_all(b"{\"op\":\"connection\",\"connectionId\":\"test\"}\r\n")
        .await
        .unwrap();

    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();

    (reader, write_half)
}

pub fn create_test_http_client(addr: SocketAddr) -> BetfairHttpClient {
    BetfairHttpClient::new(test_credential(), Some(10), Some(1), Some(100), None)
        .unwrap()
        .with_urls(
            format!("http://{addr}/login"),
            format!("http://{addr}/betting"),
            format!("http://{addr}/accounts"),
            format!("http://{addr}/navigation"),
        )
}

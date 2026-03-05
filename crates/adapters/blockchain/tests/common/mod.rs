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

use std::{
    collections::{HashMap, VecDeque},
    net::SocketAddr,
    sync::Arc,
    time::Duration,
};

use axum::{
    Router,
    body::{Body, Bytes},
    extract::State,
    http::StatusCode,
    response::Response,
    routing::{get, post},
};
use nautilus_common::testing::wait_until_async;
use nautilus_network::http::HttpClient;
use serde_json::Value;
use tokio::{net::TcpListener, sync::Mutex};

#[derive(Clone, Debug)]
pub struct MockRpcResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: String,
}

impl MockRpcResponse {
    #[must_use]
    pub fn json(body: Value) -> Self {
        Self {
            status: StatusCode::OK.as_u16(),
            headers: HashMap::new(),
            body: body.to_string(),
        }
    }

    #[must_use]
    pub fn with_status(mut self, status: StatusCode) -> Self {
        self.status = status.as_u16();
        self
    }

    #[must_use]
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), value.into());
        self
    }
}

#[derive(Clone, Default)]
pub struct MockRpcState {
    request_log: Arc<Mutex<Vec<Value>>>,
    method_counts: Arc<Mutex<HashMap<String, usize>>>,
    responses: Arc<Mutex<HashMap<String, VecDeque<MockRpcResponse>>>>,
}

impl MockRpcState {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn enqueue_json(&self, method: &str, body: Value) {
        self.enqueue_response(method, MockRpcResponse::json(body))
            .await;
    }

    pub async fn enqueue_response(&self, method: &str, response: MockRpcResponse) {
        let mut responses = self.responses.lock().await;
        responses
            .entry(method.to_string())
            .or_default()
            .push_back(response);
    }

    pub async fn request_log(&self) -> Vec<Value> {
        self.request_log.lock().await.clone()
    }

    pub async fn method_count(&self, method: &str) -> usize {
        *self.method_counts.lock().await.get(method).unwrap_or(&0)
    }

    async fn pop_response(&self, method: &str) -> Option<MockRpcResponse> {
        self.responses
            .lock()
            .await
            .get_mut(method)
            .and_then(VecDeque::pop_front)
    }

    async fn record_request(&self, method: &str, request: Value) {
        self.request_log.lock().await.push(request);
        let mut counts = self.method_counts.lock().await;
        *counts.entry(method.to_string()).or_insert(0) += 1;
    }
}

pub async fn start_mock_rpc_server(state: MockRpcState) -> SocketAddr {
    async fn health() -> &'static str {
        "OK"
    }

    async fn rpc(State(state): State<MockRpcState>, body: Bytes) -> Response<Body> {
        let request: Value = match serde_json::from_slice(&body) {
            Ok(value) => value,
            Err(e) => {
                return Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .body(Body::from(format!("{{\"error\":\"invalid json: {e}\"}}")))
                    .expect("bad-request response");
            }
        };

        let method = request
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();

        state.record_request(method.as_str(), request.clone()).await;

        let response = state
            .pop_response(method.as_str())
            .await
            .unwrap_or_else(|| {
                let request_id = request.get("id").cloned().unwrap_or(Value::Null);
                MockRpcResponse::json(serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": request_id,
                    "error": {
                        "code": -32000,
                        "message": format!("No mock response configured for method {method}"),
                    }
                }))
                .with_status(StatusCode::INTERNAL_SERVER_ERROR)
            });

        let status =
            StatusCode::from_u16(response.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let mut builder = Response::builder().status(status);
        builder = builder.header("content-type", "application/json");

        for (header, value) in response.headers {
            builder = builder.header(header, value);
        }

        builder
            .body(Body::from(response.body))
            .expect("rpc response")
    }

    let router = Router::new()
        .route("/", post(rpc))
        .route("/health", get(health))
        .with_state(state);

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind mock server");
    let addr = listener.local_addr().expect("mock server addr");

    tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("mock rpc server failed");
    });

    wait_for_server(addr).await;
    addr
}

pub async fn wait_for_server(addr: SocketAddr) {
    let url = format!("http://{addr}/health");
    let client = HttpClient::new(HashMap::new(), Vec::new(), Vec::new(), None, None, None)
        .expect("health client");

    wait_until_async(
        || {
            let health_url = url.clone();
            let http_client = client.clone();
            async move {
                http_client
                    .get(health_url, None, None, Some(1), None)
                    .await
                    .is_ok()
            }
        },
        Duration::from_secs(5),
    )
    .await;
}

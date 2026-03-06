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
use serde_json::Value;
use tokio::{net::TcpListener, sync::Mutex};

#[derive(Clone, Debug)]
pub struct MockSignerResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: String,
    pub delay_ms: u64,
}

impl MockSignerResponse {
    #[must_use]
    pub fn json(body: Value) -> Self {
        Self {
            status: StatusCode::OK.as_u16(),
            headers: HashMap::new(),
            body: body.to_string(),
            delay_ms: 0,
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

    #[must_use]
    pub fn with_delay_ms(mut self, delay_ms: u64) -> Self {
        self.delay_ms = delay_ms;
        self
    }
}

#[derive(Clone, Default)]
pub struct MockSignerState {
    request_log: Arc<Mutex<Vec<Value>>>,
    call_count: Arc<Mutex<usize>>,
    responses: Arc<Mutex<VecDeque<MockSignerResponse>>>,
}

impl MockSignerState {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn enqueue_response(&self, response: MockSignerResponse) {
        self.responses.lock().await.push_back(response);
    }

    pub async fn request_log(&self) -> Vec<Value> {
        self.request_log.lock().await.clone()
    }

    pub async fn call_count(&self) -> usize {
        *self.call_count.lock().await
    }

    async fn pop_response(&self) -> Option<MockSignerResponse> {
        self.responses.lock().await.pop_front()
    }

    async fn record_request(&self, request: Value) {
        self.request_log.lock().await.push(request);
        let mut count = self.call_count.lock().await;
        *count += 1;
    }
}

pub async fn start_mock_signer_server(state: MockSignerState) -> SocketAddr {
    async fn health() -> &'static str {
        "OK"
    }

    async fn sign_eth(State(state): State<MockSignerState>, body: Bytes) -> Response<Body> {
        let request: Value = match serde_json::from_slice(&body) {
            Ok(value) => value,
            Err(e) => {
                return Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .body(Body::from(format!("{{\"error\":\"invalid json: {e}\"}}")))
                    .expect("bad-request response");
            }
        };

        state.record_request(request).await;

        let response = state.pop_response().await.unwrap_or_else(|| {
            MockSignerResponse::json(serde_json::json!({
                "error": "No mock signer response configured",
            }))
            .with_status(StatusCode::INTERNAL_SERVER_ERROR)
        });

        if response.delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(response.delay_ms)).await;
        }

        let status =
            StatusCode::from_u16(response.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let mut builder = Response::builder().status(status);
        builder = builder.header("content-type", "application/json");

        for (header, value) in response.headers {
            builder = builder.header(header, value);
        }

        builder
            .body(Body::from(response.body))
            .expect("signer response")
    }

    let router = Router::new()
        .route("/sign/eth", post(sign_eth))
        .route("/health", get(health))
        .with_state(state);

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind mock signer server");
    let addr = listener.local_addr().expect("mock signer server addr");

    tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("mock signer server failed");
    });

    super::wait_for_server(addr).await;
    addr
}

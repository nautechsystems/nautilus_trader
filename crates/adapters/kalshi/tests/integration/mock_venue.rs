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

//! A mock of the Kalshi order API.
//!
//! The mock verifies the request signature the exchange would verify and records what it was sent, so
//! a client that refuses a command, signs the wrong path, or reads the wrong endpoint fails here
//! rather than against the exchange.

use std::{collections::VecDeque, net::SocketAddr, sync::Arc};

use aws_lc_rs::{
    rsa::KeyPair,
    signature::{KeyPair as _, RSA_PSS_2048_8192_SHA256, UnparsedPublicKey},
};
use axum::{
    Router,
    body::{Body, Bytes},
    extract::{Path, State},
    http::{HeaderMap, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use parking_lot::Mutex;

use crate::harness::{CLIENT_ORDER_ID, VENUE_ORDER_ID, order_json, test_private_key_pem};

/// What the mock venue holds and what it observed.
///
/// The response queues hold the status codes the venue returns before it answers normally, which is
/// how the tests reproduce a venue whose read path lags its write path and whose requests fail.
#[derive(Clone, Debug)]
pub(crate) struct MockVenue {
    pub(crate) order: Arc<Mutex<String>>,
    pub(crate) fills: Arc<Mutex<Vec<String>>>,
    pub(crate) creates: Arc<Mutex<Vec<serde_json::Value>>>,
    pub(crate) signs: Arc<Mutex<Vec<String>>>,
    pub(crate) order_responses: Arc<Mutex<VecDeque<StatusCode>>>,
    pub(crate) fill_responses: Arc<Mutex<VecDeque<StatusCode>>>,
    pub(crate) create_responses: Arc<Mutex<VecDeque<StatusCode>>>,
    pub(crate) cutoff: Arc<Mutex<Option<String>>>,
    pub(crate) cancel_deletes: Arc<Mutex<usize>>,
}

impl MockVenue {
    pub(crate) fn new(order: String, fills: Vec<String>) -> Self {
        Self {
            order: Arc::new(Mutex::new(order)),
            fills: Arc::new(Mutex::new(fills)),
            creates: Arc::new(Mutex::new(Vec::new())),
            signs: Arc::new(Mutex::new(Vec::new())),
            order_responses: Arc::new(Mutex::new(VecDeque::new())),
            fill_responses: Arc::new(Mutex::new(VecDeque::new())),
            create_responses: Arc::new(Mutex::new(VecDeque::new())),
            cutoff: Arc::new(Mutex::new(None)),
            cancel_deletes: Arc::new(Mutex::new(0)),
        }
    }

    pub(crate) fn resting() -> Self {
        Self::new(order_json("resting", "0.00", "100.00"), Vec::new())
    }

    pub(crate) fn set_order(&self, order: String) {
        *self.order.lock() = order;
    }

    pub(crate) fn add_fill(&self, fill: &str) {
        self.fills.lock().push(fill.to_string());
    }

    pub(crate) fn creates(&self) -> Vec<serde_json::Value> {
        self.creates.lock().clone()
    }

    /// Records that the venue received a cancellation.
    pub(crate) fn note_cancel(&self) -> usize {
        let mut deletes = self.cancel_deletes.lock();

        *deletes += 1;
        *deletes
    }

    pub(crate) fn signed(&self, method_and_path: &str) -> bool {
        self.signs.lock().iter().any(|seen| seen == method_and_path)
    }

    /// Returns the next response for each request that the tests inject faults into.
    pub(crate) fn take(queue: &Arc<Mutex<VecDeque<StatusCode>>>) -> Option<StatusCode> {
        queue.lock().pop_front()
    }
}

pub(crate) async fn record_signature(
    State(venue): State<MockVenue>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let headers = request.headers().clone();

    if let (Some(timestamp), Some(signature)) = (
        headers
            .get("KALSHI-ACCESS-TIMESTAMP")
            .and_then(|v| v.to_str().ok()),
        headers
            .get("KALSHI-ACCESS-SIGNATURE")
            .and_then(|v| v.to_str().ok()),
    ) {
        let method = request.method().as_str().to_string();
        let path = request.uri().path().to_string();
        let message = format!("{timestamp}{method}{path}");

        assert!(
            verify(&message, signature),
            "signature did not verify for message '{message}'"
        );
        venue.signs.lock().push(format!("{method} {path}"));
    }

    next.run(request).await
}

pub(crate) fn verify(message: &str, signature: &str) -> bool {
    let pem = pem::parse(test_private_key_pem().trim()).expect("fixture is PEM");
    let key_pair = KeyPair::from_pkcs8(pem.contents()).expect("fixture is PKCS#8 RSA");
    let public_key = key_pair.public_key();

    let Ok(decoded) = STANDARD.decode(signature) else {
        return false;
    };

    UnparsedPublicKey::new(&RSA_PSS_2048_8192_SHA256, public_key.as_ref())
        .verify(message.as_bytes(), &decoded)
        .is_ok()
}

pub(crate) async fn exchange_status() -> Response {
    (
        StatusCode::OK,
        r#"{"exchange_active": true, "trading_active": true}"#,
    )
        .into_response()
}

pub(crate) async fn balance(headers: HeaderMap) -> Response {
    if !headers.contains_key("KALSHI-ACCESS-KEY") {
        return (
            StatusCode::UNAUTHORIZED,
            r#"{"code":"missing_key","message":"authentication required"}"#,
        )
            .into_response();
    }

    (
        StatusCode::OK,
        r#"{
            "balance": 412500,
            "balance_dollars": "4125.0000",
            "portfolio_value": 500000,
            "updated_ts": 1735732800
        }"#,
    )
        .into_response()
}

pub(crate) async fn positions() -> Response {
    (
        StatusCode::OK,
        r#"{
            "cursor": "",
            "market_positions": [{
                "ticker": "KXHIGHNY-25JAN01-T50",
                "exchange_index": 0,
                "total_traded_dollars": "34.0000",
                "position_fp": "100.00",
                "market_exposure_dollars": "34.0000",
                "realized_pnl_dollars": "0.0000",
                "fees_paid_dollars": "0.1000",
                "last_updated_ts": "2025-01-01T12:00:05Z"
            }],
            "event_positions": []
        }"#,
    )
        .into_response()
}

pub(crate) async fn orders(State(venue): State<MockVenue>) -> Response {
    let order = venue.order.lock().clone();

    (
        StatusCode::OK,
        format!(r#"{{"orders": [{order}], "cursor": ""}}"#),
    )
        .into_response()
}

pub(crate) async fn order(
    State(venue): State<MockVenue>,
    Path(_order_id): Path<String>,
) -> Response {
    if let Some(status) = MockVenue::take(&venue.order_responses) {
        return (status, r#"{"code":"not_found","message":"no such order"}"#).into_response();
    }

    let order = venue.order.lock().clone();

    (StatusCode::OK, format!(r#"{{"order": {order}}}"#)).into_response()
}

pub(crate) async fn fills(State(venue): State<MockVenue>) -> Response {
    if let Some(status) = MockVenue::take(&venue.fill_responses) {
        return (status, r#"{"code":"unavailable","message":"try later"}"#).into_response();
    }

    let fills = venue.fills.lock().join(", ");

    (
        StatusCode::OK,
        format!(r#"{{"fills": [{fills}], "cursor": ""}}"#),
    )
        .into_response()
}

pub(crate) async fn historical_cutoff(State(venue): State<MockVenue>) -> Response {
    let cutoff = venue.cutoff.lock().clone();

    match cutoff {
        Some(cutoff) => (
            StatusCode::OK,
            format!(
                r#"{{
                    "market_settled_ts": "{cutoff}",
                    "trades_created_ts": "{cutoff}",
                    "orders_updated_ts": "{cutoff}",
                    "market_positions_last_updated_ts": "{cutoff}"
                }}"#
            ),
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, r#"{"code":"not_found"}"#).into_response(),
    }
}

pub(crate) async fn create_order(State(venue): State<MockVenue>, body: Bytes) -> Response {
    let request: serde_json::Value = serde_json::from_slice(&body).expect("create body is JSON");

    venue.creates.lock().push(request);

    if let Some(status) = MockVenue::take(&venue.create_responses) {
        return (
            status,
            r#"{"code":"unavailable","message":"the request was not answered"}"#,
        )
            .into_response();
    }

    (
        StatusCode::OK,
        format!(
            r#"{{
                "order_id": "{VENUE_ORDER_ID}",
                "client_order_id": "{CLIENT_ORDER_ID}",
                "fill_count": "0.00",
                "remaining_count": "100.00",
                "average_fill_price": null,
                "average_fee_paid": null,
                "ts_ms": 1735732800000
            }}"#
        ),
    )
        .into_response()
}

pub(crate) async fn cancel_order(
    State(venue): State<MockVenue>,
    Path(_order_id): Path<String>,
) -> Response {
    venue.note_cancel();

    (
        StatusCode::OK,
        format!(
            r#"{{
                "order_id": "{VENUE_ORDER_ID}",
                "client_order_id": "{CLIENT_ORDER_ID}",
                "reduced_by": "100.00",
                "ts_ms": 1735732801000
            }}"#
        ),
    )
        .into_response()
}

pub(crate) async fn amend_order(Path(_order_id): Path<String>) -> Response {
    (
        StatusCode::OK,
        format!(
            r#"{{
                "order_id": "{VENUE_ORDER_ID}",
                "client_order_id": "{CLIENT_ORDER_ID}",
                "remaining_count": "50.00",
                "fill_count": "50.00",
                "average_fill_price": "0.3400",
                "average_fee_paid": "0.1000",
                "ts_ms": 1735732802000
            }}"#
        ),
    )
        .into_response()
}

/// Starts the mock venue and returns its address.
pub(crate) async fn spawn_mock(venue: MockVenue) -> SocketAddr {
    let router = Router::new()
        .route("/trade-api/v2/exchange/status", get(exchange_status))
        .route("/trade-api/v2/portfolio/balance", get(balance))
        .route("/trade-api/v2/portfolio/positions", get(positions))
        .route("/trade-api/v2/portfolio/orders", get(orders))
        .route("/trade-api/v2/portfolio/orders/{order_id}", get(order))
        .route("/trade-api/v2/portfolio/fills", get(fills))
        .route("/trade-api/v2/historical/cutoff", get(historical_cutoff))
        .route("/trade-api/v2/portfolio/events/orders", post(create_order))
        .route(
            "/trade-api/v2/portfolio/events/orders/{order_id}",
            delete(cancel_order),
        )
        .route(
            "/trade-api/v2/portfolio/events/orders/{order_id}/amend",
            post(amend_order),
        )
        .layer(middleware::from_fn_with_state(
            venue.clone(),
            record_signature,
        ))
        .with_state(venue);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    addr
}

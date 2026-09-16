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

//! HTTP client tests against a mock of the Kalshi Trade API.
//!
//! The mock verifies the request signature the exchange would verify, so a client that signs the
//! wrong path, or omits a header, fails here rather than in production.

use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use aws_lc_rs::{
    rsa::KeyPair,
    signature::{KeyPair as _, RSA_PSS_2048_8192_SHA256, UnparsedPublicKey},
};
use axum::{
    Router,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use nautilus_kalshi::{
    KalshiHttpClient,
    common::{credential::KalshiCredential, enums::KalshiMarketStatus},
    http::{auth::KalshiAuth, error::Error},
};
use nautilus_model::instruments::InstrumentAny;
use parking_lot::Mutex;

use crate::harness::test_private_key_pem;

/// A market payload shaped like the exchange's own response.
const MARKET_JSON: &str = r#"{
    "ticker": "KXHIGHNY-25JAN01-T50",
    "event_ticker": "KXHIGHNY-25JAN01",
    "market_type": "binary",
    "yes_sub_title": "50 degrees or above",
    "no_sub_title": "49 degrees or below",
    "created_time": "2024-12-30T15:00:00Z",
    "updated_time": "2025-01-01T06:00:00Z",
    "open_time": "2024-12-30T15:00:00Z",
    "close_time": "2025-01-02T05:00:00Z",
    "latest_expiration_time": "2025-01-05T05:00:00Z",
    "settlement_timer_seconds": 1800,
    "status": "active",
    "notional_value_dollars": "1.0000",
    "yes_bid_dollars": "0.3400",
    "yes_ask_dollars": "0.3500",
    "no_bid_dollars": "0.6500",
    "no_ask_dollars": "0.6600",
    "yes_bid_size_fp": "120.00",
    "yes_ask_size_fp": "80.00",
    "last_price_dollars": "0.3500",
    "previous_yes_bid_dollars": "0.3300",
    "previous_yes_ask_dollars": "0.3600",
    "previous_price_dollars": "0.3400",
    "volume_fp": "1520.00",
    "volume_24h_fp": "310.00",
    "open_interest_fp": "900.00",
    "result": "",
    "can_close_early": true,
    "expiration_value": "51",
    "rules_primary": "Resolves YES if the high is 50 or above.",
    "rules_secondary": "Source: NWS Central Park.",
    "price_level_structure": "linear_cent",
    "price_ranges": [
        {"start": "0.0000", "end": "1.0000", "step": "0.0100"}
    ]
}"#;

/// Records what the mock server observed, so tests can assert on the request as sent.
#[derive(Debug, Default)]
struct Observed {
    signed_requests: Mutex<Vec<String>>,
    api_key: Mutex<Option<String>>,
}

#[derive(Clone)]
struct MockState {
    observed: Arc<Observed>,
    /// When set, the balance endpoint responds with this status instead of a balance.
    balance_failure: Option<StatusCode>,
}

async fn record_signature(
    State(state): State<MockState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let headers = request.headers().clone();

    if let Some(api_key) = headers
        .get("KALSHI-ACCESS-KEY")
        .and_then(|v| v.to_str().ok())
    {
        *state.observed.api_key.lock() = Some(api_key.to_string());
    }

    if let (Some(timestamp), Some(signature)) = (
        headers
            .get("KALSHI-ACCESS-TIMESTAMP")
            .and_then(|v| v.to_str().ok()),
        headers
            .get("KALSHI-ACCESS-SIGNATURE")
            .and_then(|v| v.to_str().ok()),
    ) {
        let path = request.uri().path().to_string();
        let message = format!("{timestamp}{}{path}", request.method().as_str());

        assert!(
            verify(&message, signature),
            "signature did not verify for message '{message}'"
        );
        state.observed.signed_requests.lock().push(message);
    }

    next.run(request).await
}

fn verify(message: &str, signature: &str) -> bool {
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

async fn markets(Query(params): Query<HashMap<String, String>>) -> Response {
    // The first page carries a cursor; asking for it returns the second page and no cursor.
    if params.get("cursor").map(String::as_str) == Some("page-2") {
        let raw = MARKET_JSON.replace("KXHIGHNY-25JAN01-T50", "KXHIGHNY-25JAN01-T60");

        return (
            StatusCode::OK,
            format!(r#"{{"markets": [{raw}], "cursor": ""}}"#),
        )
            .into_response();
    }

    (
        StatusCode::OK,
        format!(r#"{{"markets": [{MARKET_JSON}], "cursor": "page-2"}}"#),
    )
        .into_response()
}

async fn market(Path(_ticker): Path<String>) -> Response {
    (StatusCode::OK, format!(r#"{{"market": {MARKET_JSON}}}"#)).into_response()
}

async fn orderbook(Path(_ticker): Path<String>) -> Response {
    (
        StatusCode::OK,
        r#"{"orderbook_fp": {"yes_dollars": [["0.3400", "120.00"]], "no_dollars": [["0.6500", "60.00"]]}}"#,
    )
        .into_response()
}

async fn balance(State(state): State<MockState>, headers: HeaderMap) -> Response {
    if let Some(status) = state.balance_failure {
        return (
            status,
            r#"{"code":"auth_error","message":"invalid signature"}"#,
        )
            .into_response();
    }

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

async fn positions() -> Response {
    (
        StatusCode::OK,
        r#"{
            "cursor": "",
            "market_positions": [{
                "ticker": "KXHIGHNY-25JAN01-T50",
                "exchange_index": 0,
                "total_traded_dollars": "350.0000",
                "position_fp": "100.00",
                "market_exposure_dollars": "350.0000",
                "realized_pnl_dollars": "0.0000",
                "fees_paid_dollars": "1.7500",
                "last_updated_ts": "2025-01-01T12:00:00Z"
            }],
            "event_positions": []
        }"#,
    )
        .into_response()
}

async fn spawn_mock(balance_failure: Option<StatusCode>) -> (SocketAddr, Arc<Observed>) {
    let observed = Arc::new(Observed::default());
    let state = MockState {
        observed: Arc::clone(&observed),
        balance_failure,
    };
    let router = Router::new()
        .route("/trade-api/v2/markets", get(markets))
        .route("/trade-api/v2/markets/{ticker}", get(market))
        .route("/trade-api/v2/markets/{ticker}/orderbook", get(orderbook))
        .route("/trade-api/v2/portfolio/balance", get(balance))
        .route("/trade-api/v2/portfolio/positions", get(positions))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            record_signature,
        ))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    (addr, observed)
}

fn client(addr: SocketAddr, authenticated: bool) -> KalshiHttpClient {
    let auth = authenticated.then(|| {
        KalshiAuth::new(KalshiCredential::new(
            "a952bcbe-ec3b-4b5b-b8f9-11dae589608c".to_string(),
            test_private_key_pem().to_string(),
        ))
    });

    KalshiHttpClient::new(
        Some(format!("http://{addr}/trade-api/v2")),
        Some(5),
        None,
        auth,
    )
    .unwrap()
}

#[tokio::test]
async fn test_markets_pagination_follows_the_cursor_to_the_last_page() {
    let (addr, _) = spawn_mock(None).await;
    let markets = client(addr, false)
        .get_all_markets(Some(KalshiMarketStatus::Active), None, None)
        .await
        .unwrap();

    assert_eq!(markets.len(), 2);
    assert_eq!(markets[0].ticker, "KXHIGHNY-25JAN01-T50");
    assert_eq!(markets[1].ticker, "KXHIGHNY-25JAN01-T60");
}

#[tokio::test]
async fn test_market_and_orderbook_decode_into_domain_values() {
    let (addr, _) = spawn_mock(None).await;
    let client = client(addr, false);
    let market = client.get_market("KXHIGHNY-25JAN01-T50").await.unwrap();
    let book = client
        .get_market_orderbook("KXHIGHNY-25JAN01-T50", None)
        .await
        .unwrap();

    assert_eq!(market.ticker, "KXHIGHNY-25JAN01-T50");
    assert_eq!(book.yes_dollars.len(), 1);
    assert_eq!(book.no_dollars[0].1, "60.00");

    let instrument = nautilus_kalshi::http::parse::create_instrument_from_market(
        &market,
        nautilus_core::UnixNanos::from(1),
    )
    .unwrap();

    assert!(matches!(instrument, InstrumentAny::BinaryOption(_)));
}

#[tokio::test]
async fn test_authenticated_request_signs_the_traded_path() {
    let (addr, observed) = spawn_mock(None).await;
    let balance = client(addr, true).get_balance().await.unwrap();

    assert_eq!(balance.balance, 412_500);
    assert_eq!(balance.balance_dollars, "4125.0000");
    assert_eq!(
        observed.api_key.lock().as_deref(),
        Some("a952bcbe-ec3b-4b5b-b8f9-11dae589608c")
    );

    let signed = observed.signed_requests.lock().clone();

    assert_eq!(signed.len(), 1);
    assert!(
        signed[0].ends_with("GET/trade-api/v2/portfolio/balance"),
        "{signed:?}"
    );
}

#[tokio::test]
async fn test_unauthenticated_request_is_refused_before_it_is_sent() {
    let (addr, observed) = spawn_mock(None).await;
    let error = client(addr, false).get_balance().await.unwrap_err();

    assert!(matches!(error, Error::MissingCredential(_)));
    assert!(observed.signed_requests.lock().is_empty());
}

#[tokio::test]
async fn test_positions_are_fetched_across_the_whole_account() {
    let (addr, _) = spawn_mock(None).await;
    let positions = client(addr, true).get_all_positions().await.unwrap();

    assert_eq!(positions.market_positions.len(), 1);
    assert_eq!(positions.market_positions[0].position_fp, "100.00");
    assert_eq!(positions.market_positions[0].realized_pnl_dollars, "0.0000");
}

#[tokio::test]
async fn test_unauthorized_response_reports_the_exchange_error() {
    let (addr, _) = spawn_mock(Some(StatusCode::UNAUTHORIZED)).await;
    let error = client(addr, true).get_balance().await.unwrap_err();

    assert!(matches!(error, Error::Http { status: 401, .. }));
    assert!(error.to_string().contains("invalid signature"), "{error}");
    assert!(!error.is_retryable());
}

#[tokio::test]
async fn test_rate_limited_response_is_reported_as_retryable() {
    let (addr, _) = spawn_mock(Some(StatusCode::TOO_MANY_REQUESTS)).await;
    let error = client(addr, true).get_balance().await.unwrap_err();

    assert!(matches!(error, Error::RateLimited { .. }));
    assert!(error.is_retryable());
}

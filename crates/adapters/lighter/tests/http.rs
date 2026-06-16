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

//! Integration tests for the Lighter HTTP client using a mock Axum server.

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use axum::{
    Router,
    body::Bytes,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use chrono::{TimeZone, Utc};
use nautilus_core::UnixNanos;
use nautilus_lighter::{
    common::enums::{
        LighterCandleResolution, LighterEnvironment, LighterFundingResolution, LighterMarketStatus,
        LighterOrderBookFilter, LighterTxType,
    },
    http::{
        client::{
            LIGHTER_CANDLES_MAX_LIMIT, LIGHTER_FUNDINGS_MAX_LIMIT, LIGHTER_REST_PAGE_SIZE,
            LighterHttpClient, LighterRawHttpClient,
        },
        error::LighterHttpError,
        models::{LighterSendTxBatchRequest, LighterSendTxRequest},
        query::{
            LighterAccountActiveOrdersQuery, LighterAccountActiveOrdersQueryBuilder,
            LighterAccountInactiveOrdersQuery, LighterAccountInactiveOrdersQueryBuilder,
            LighterAccountLookup, LighterAccountQuery, LighterCandlesQuery,
            LighterCandlesQueryBuilder, LighterFundingsQuery, LighterMakerOnlyApiKeysQueryBuilder,
            LighterNextNonceQuery, LighterOrderBookDetailsQuery,
            LighterOrderBookDetailsQueryBuilder, LighterOrderBookOrdersQuery,
            LighterOrderBooksQuery, LighterOrderBooksQueryBuilder, LighterRecentTradesQuery,
            LighterSortDirection, LighterTradeQueryType, LighterTradeRole, LighterTradeSortBy,
            LighterTradesQuery, LighterTradesQueryBuilder,
        },
    },
};
use nautilus_model::{
    data::{BarSpecification, BarType},
    enums::{
        AggregationSource, AggressorSide, BarAggregation, BookAction, OrderSide, PriceType,
        RecordFlag,
    },
    identifiers::{InstrumentId, Symbol, Venue},
    instruments::{CryptoPerpetual, Instrument, InstrumentAny},
    types::{Price, Quantity, currency::Currency},
};
use nautilus_network::retry::{RetryConfig, RetryManager};
use rust_decimal::Decimal;

const HTTP_NEXT_NONCE: &str = include_str!("../test_data/http_next_nonce.json");
const HTTP_ORDER_BOOK_DETAILS: &str = include_str!("../test_data/http_order_book_details.json");
const HTTP_ORDER_BOOK_ORDERS: &str = include_str!("../test_data/http_order_book_orders.json");
const HTTP_ORDER_BOOKS: &str = include_str!("../test_data/http_order_books.json");
const HTTP_ORDERS: &str = include_str!("../test_data/http_orders.json");
const HTTP_RECENT_TRADES: &str = include_str!("../test_data/http_recent_trades.json");
const HTTP_CANDLES: &str = include_str!("../test_data/http_candles.json");
const HTTP_FUNDINGS: &str = include_str!("../test_data/http_fundings.json");
const HTTP_ACCOUNT: &str = include_str!("../test_data/http_account.json");
const MINUTE_MS: i64 = 60_000;

#[derive(Clone)]
struct IncompleteCandlesState {
    completed_start_ms: i64,
    incomplete_start_ms: i64,
}

#[derive(Clone)]
struct LatestCandlesState {
    end_ms: i64,
}

#[derive(Clone)]
struct PaginatedCandlesState {
    start_ms: i64,
    calls: Arc<AtomicUsize>,
}

#[derive(Clone)]
struct PaginatedFundingsState {
    start_ms: i64,
    calls: Arc<AtomicUsize>,
}

#[derive(Clone)]
struct LatestFundingsState {
    end_ms: i64,
}

#[tokio::test]
async fn raw_client_get_order_books_sends_query_and_parses_response() {
    let base_url =
        spawn_server(Router::new().route("/api/v1/orderBooks", get(handle_order_books))).await;
    let client =
        LighterRawHttpClient::new(LighterEnvironment::Mainnet, Some(base_url), 10, None).unwrap();
    let query = LighterOrderBooksQueryBuilder::default()
        .market_id(0)
        .filter(LighterOrderBookFilter::Perp)
        .build()
        .unwrap();

    let response = client.get_order_books(&query).await.unwrap();

    assert_eq!(response.code, 200);
    assert_eq!(response.order_books.len(), 1);
    assert_eq!(response.order_books[0].market_id, 0);
}

#[tokio::test]
async fn raw_client_get_order_book_details_sends_query_and_parses_response() {
    let base_url = spawn_server(Router::new().route(
        "/api/v1/orderBookDetails",
        get(handle_order_book_details_filtered),
    ))
    .await;
    let client = raw_client(base_url);
    let query = LighterOrderBookDetailsQueryBuilder::default()
        .market_id(0)
        .filter(LighterOrderBookFilter::Perp)
        .build()
        .unwrap();

    let response = client.get_order_book_details(&query).await.unwrap();

    assert_eq!(response.code, 200);
    assert_eq!(response.order_book_details.len(), 1);
    assert_eq!(response.order_book_details[0].order_book.market_id, 0);
}

#[tokio::test]
async fn raw_client_get_order_book_orders_sends_query_and_parses_response() {
    let base_url =
        spawn_server(Router::new().route("/api/v1/orderBookOrders", get(handle_order_book_orders)))
            .await;
    let client = raw_client(base_url);
    let query = LighterOrderBookOrdersQuery {
        market_id: 0,
        limit: 25,
    };

    let response = client.get_order_book_orders(&query).await.unwrap();

    assert_eq!(response.code, 200);
    assert_eq!(response.asks.len(), 1);
    assert_eq!(response.bids.len(), 1);
}

#[tokio::test]
async fn raw_client_get_trades_sends_paginated_query_and_parses_response() {
    let base_url = spawn_server(Router::new().route("/api/v1/trades", get(handle_trades))).await;
    let client = raw_client(base_url);
    let query = LighterTradesQueryBuilder::default()
        .authorization("bearer-token")
        .market_id(0)
        .account_index(712_440)
        .order_index(281_476_929_510_110)
        .sort_by(LighterTradeSortBy::Timestamp)
        .sort_dir(LighterSortDirection::Desc)
        .cursor("cursor-1")
        .from_timestamp(1_700_000_000_000)
        .ask_filter(1)
        .role(LighterTradeRole::Maker)
        .trade_type(LighterTradeQueryType::Trade)
        .limit(50)
        .aggregate(true)
        .build()
        .unwrap();

    let response = client.get_trades(&query).await.unwrap();

    assert_eq!(response.code, 200);
    assert_eq!(response.trades.len(), 1);
}

#[tokio::test]
async fn raw_client_get_candles_sends_query_and_parses_response() {
    let base_url = spawn_server(Router::new().route("/api/v1/candles", get(handle_candles))).await;
    let client = raw_client(base_url);
    let query = LighterCandlesQueryBuilder::default()
        .market_id(0)
        .resolution(LighterCandleResolution::OneMinute)
        .start_timestamp(1_700_000_000_000)
        .end_timestamp(1_700_000_120_000)
        .count_back(i64::from(LIGHTER_CANDLES_MAX_LIMIT))
        .set_timestamp_to_end(false)
        .build()
        .unwrap();

    let response = client.get_candles(&query).await.unwrap();

    assert_eq!(response.code, 200);
    assert_eq!(response.resolution, LighterCandleResolution::OneMinute);
    assert_eq!(response.candles.len(), 2);
    assert_eq!(response.candles[0].timestamp, 1_700_000_000_000);
}

#[tokio::test]
async fn raw_client_get_fundings_sends_query_and_parses_response() {
    let base_url =
        spawn_server(Router::new().route("/api/v1/fundings", get(handle_fundings))).await;
    let client = raw_client(base_url);
    let query = LighterFundingsQuery {
        market_id: 0,
        resolution: LighterFundingResolution::OneHour,
        start_timestamp: 1_778_702_400_000,
        end_timestamp: 1_778_706_000_000,
        count_back: 2,
    };

    let response = client.get_fundings(&query).await.unwrap();

    assert_eq!(response.code, 200);
    assert_eq!(response.resolution, LighterFundingResolution::OneHour);
    assert_eq!(response.fundings.len(), 2);
}

#[tokio::test]
async fn raw_client_get_trades_serializes_lighter_rest_page_size() {
    // Anchors the venue's `limit <= 100` contract through URL serialization.
    let base_url = spawn_server(
        Router::new().route("/api/v1/trades", get(handle_trades_with_page_size_limit)),
    )
    .await;
    let client = raw_client(base_url);
    let query = LighterTradesQueryBuilder::default()
        .auth("auth-token")
        .market_id(0)
        .account_index(712_440)
        .sort_by(LighterTradeSortBy::Timestamp)
        .sort_dir(LighterSortDirection::Desc)
        .limit(LIGHTER_REST_PAGE_SIZE)
        .build()
        .unwrap();

    let response = client.get_trades(&query).await.unwrap();

    assert_eq!(response.code, 200);
    assert_eq!(LIGHTER_REST_PAGE_SIZE, 100); // venue contract.
}

#[tokio::test]
async fn raw_client_get_account_orders_sends_auth_queries_and_parses_response() {
    let base_url = spawn_server(
        Router::new()
            .route(
                "/api/v1/accountActiveOrders",
                get(handle_account_active_orders),
            )
            .route(
                "/api/v1/accountInactiveOrders",
                get(handle_account_inactive_orders),
            ),
    )
    .await;
    let client = raw_client(base_url);
    let active_query = LighterAccountActiveOrdersQueryBuilder::default()
        .auth("auth-token")
        .account_index(712_440)
        .market_id(0)
        .build()
        .unwrap();
    let inactive_query = LighterAccountInactiveOrdersQueryBuilder::default()
        .authorization("bearer-token")
        .account_index(712_440)
        .market_id(0)
        .ask_filter(1)
        .between_timestamps("1700000000000,1700000001000")
        .cursor("cursor-1")
        .limit(50)
        .build()
        .unwrap();

    let active = client
        .get_account_active_orders(&active_query)
        .await
        .unwrap();
    let inactive = client
        .get_account_inactive_orders(&inactive_query)
        .await
        .unwrap();

    assert_eq!(active.orders.len(), 1);
    assert_eq!(inactive.next_cursor.as_deref(), Some("cursor-1"));
}

#[tokio::test]
async fn raw_client_get_next_nonce_sends_query_and_parses_response() {
    let base_url =
        spawn_server(Router::new().route("/api/v1/nextNonce", get(handle_next_nonce))).await;
    let client = raw_client(base_url);
    let query = LighterNextNonceQuery {
        account_index: 12_345,
        api_key_index: 5,
    };

    let response = client.get_next_nonce(&query).await.unwrap();

    assert_eq!(response.code, 200);
    assert_eq!(response.nonce, 1_234_567_890);
}

#[tokio::test]
async fn domain_client_get_maker_only_api_keys_sends_authorization_header() {
    let base_url = spawn_server(Router::new().route(
        "/api/v1/getMakerOnlyApiKeys",
        get(handle_maker_only_api_keys),
    ))
    .await;
    let client =
        LighterHttpClient::new(LighterEnvironment::Mainnet, Some(base_url), 10, None).unwrap();

    let response = client
        .get_maker_only_api_keys(712_440, "auth-token")
        .await
        .unwrap();

    assert_eq!(response.code, 200);
    assert_eq!(response.api_key_indexes, vec![5]);
}

#[tokio::test]
async fn raw_client_get_maker_only_api_keys_maps_auth_query_field_to_authorization_header() {
    let base_url = spawn_server(Router::new().route(
        "/api/v1/getMakerOnlyApiKeys",
        get(handle_maker_only_api_keys),
    ))
    .await;
    let client = raw_client(base_url);
    let query = LighterMakerOnlyApiKeysQueryBuilder::default()
        .auth("auth-token")
        .account_index(712_440)
        .build()
        .unwrap();

    let response = client.get_maker_only_api_keys(&query).await.unwrap();

    assert_eq!(response.code, 200);
    assert_eq!(response.api_key_indexes, vec![5]);
}

#[tokio::test]
async fn raw_client_send_tx_posts_form_and_parses_response() {
    let base_url = spawn_server(Router::new().route("/api/v1/sendTx", post(handle_send_tx))).await;
    let client = raw_client(base_url);
    let request = LighterSendTxRequest::new(
        LighterTxType::CreateOrder as u8,
        r#"{"AccountIndex":1,"Nonce":2,"Sig":"0xsig"}"#,
    )
    .with_price_protection(true);

    let response = client.send_tx(&request).await.unwrap();

    assert_eq!(response.code, 200);
    assert_eq!(response.tx_hash, "0xabc");
    assert_eq!(response.predicted_execution_time_ms, 1_751_465_474);
    assert_eq!(response.volume_quota_remaining, Some(123));
}

#[tokio::test]
async fn raw_client_send_tx_posts_false_price_protection() {
    let base_url = spawn_server(Router::new().route(
        "/api/v1/sendTx",
        post(handle_send_tx_false_price_protection),
    ))
    .await;
    let client = raw_client(base_url);
    let request = LighterSendTxRequest::new(
        LighterTxType::CreateOrder as u8,
        r#"{"AccountIndex":1,"Nonce":2,"Sig":"0xsig"}"#,
    )
    .with_price_protection(false);

    let response = client.send_tx(&request).await.unwrap();

    assert_eq!(response.code, 200);
    assert_eq!(response.tx_hash, "0xabc");
    assert_eq!(response.predicted_execution_time_ms, 1_751_465_474);
    assert_eq!(response.volume_quota_remaining, Some(123));
}

#[tokio::test]
async fn raw_client_send_tx_batch_posts_form_and_parses_response() {
    let base_url =
        spawn_server(Router::new().route("/api/v1/sendTxBatch", post(handle_send_tx_batch))).await;
    let client = raw_client(base_url);
    let request = LighterSendTxBatchRequest::new(
        format!(
            "[{},{}]",
            LighterTxType::CreateOrder as u8,
            LighterTxType::CancelOrder as u8,
        ),
        r#"[{"AccountIndex":1},{"AccountIndex":1}]"#,
    );

    let response = client.send_tx_batch(&request).await.unwrap();

    assert_eq!(response.code, 200);
    assert_eq!(
        response.tx_hash,
        vec!["0xabc".to_string(), "0xdef".to_string()]
    );
    assert_eq!(response.predicted_execution_time_ms, 1_751_465_475);
    assert_eq!(response.volume_quota_remaining, Some(122));
}

#[tokio::test]
async fn raw_client_send_tx_batch_parses_missing_volume_quota_remaining() {
    let base_url = spawn_server(Router::new().route(
        "/api/v1/sendTxBatch",
        post(handle_send_tx_batch_without_volume_quota),
    ))
    .await;
    let client = raw_client(base_url);
    let request = LighterSendTxBatchRequest::new(
        format!(
            "[{},{}]",
            LighterTxType::CreateOrder as u8,
            LighterTxType::CancelOrder as u8
        ),
        r#"[{"AccountIndex":1},{"AccountIndex":1}]"#,
    );

    let response = client.send_tx_batch(&request).await.unwrap();

    assert_eq!(response.code, 200);
    assert_eq!(response.volume_quota_remaining, None);
}

#[tokio::test]
async fn raw_client_send_tx_maps_success_body_errors() {
    let base_url = spawn_server(
        Router::new()
            .route(
                "/api/v1/sendTx",
                post(handle_send_tx_success_body_venue_error),
            )
            .route(
                "/api/v1/sendTxBatch",
                post(handle_send_tx_batch_success_body_rate_limit),
            ),
    )
    .await;
    let client = raw_client(base_url);
    let request = LighterSendTxRequest::new(
        LighterTxType::CreateOrder as u8,
        r#"{"AccountIndex":1,"Nonce":2,"Sig":"0xsig"}"#,
    );
    let batch_request = LighterSendTxBatchRequest::new("[14,15]", "[]");

    let venue_error = client.send_tx(&request).await.unwrap_err();
    let rate_limit_error = client.send_tx_batch(&batch_request).await.unwrap_err();

    assert!(matches!(
        venue_error,
        LighterHttpError::Venue { code: 1001, message } if message == "invalid tx"
    ));
    assert!(matches!(
        rate_limit_error,
        LighterHttpError::RateLimit(message) if message == "slow down"
    ));
}

#[tokio::test]
async fn raw_client_maps_rate_limit_status() {
    let base_url =
        spawn_server(Router::new().route("/api/v1/recentTrades", get(handle_rate_limit))).await;
    let client = raw_client(base_url);
    let query = LighterRecentTradesQuery {
        market_id: 0,
        limit: 10,
    };

    let error = client.get_recent_trades(&query).await.unwrap_err();

    assert!(matches!(error, LighterHttpError::RateLimit(_)));
}

#[tokio::test]
async fn raw_client_maps_structured_venue_error() {
    let base_url =
        spawn_server(Router::new().route("/api/v1/orderBooks", get(handle_venue_error))).await;
    let client = raw_client(base_url);
    let query = LighterOrderBooksQueryBuilder::default().build().unwrap();

    let error = client.get_order_books(&query).await.unwrap_err();

    assert!(matches!(
        error,
        LighterHttpError::Venue { code: 1001, message } if message == "invalid market"
    ));
}

#[tokio::test]
async fn raw_client_maps_http_method_not_allowed_status() {
    let base_url =
        spawn_server(Router::new().route("/api/v1/orderBooks", get(handle_method_not_allowed)))
            .await;
    let client = raw_client(base_url);
    let query = LighterOrderBooksQueryBuilder::default().build().unwrap();

    let error = client.get_order_books(&query).await.unwrap_err();

    assert!(matches!(error, LighterHttpError::Http { status: 405, .. }));
}

#[tokio::test]
async fn raw_client_maps_structured_rate_limit_code() {
    let base_url =
        spawn_server(Router::new().route("/api/v1/orderBooks", get(handle_body_rate_limit))).await;
    let client = raw_client(base_url);
    let query = LighterOrderBooksQueryBuilder::default().build().unwrap();

    let error = client.get_order_books(&query).await.unwrap_err();

    assert!(matches!(
        error,
        LighterHttpError::RateLimit(message) if message == "slow down"
    ));
}

#[tokio::test]
async fn raw_client_maps_success_body_errors() {
    let base_url = spawn_server(
        Router::new()
            .route("/api/v1/orderBooks", get(handle_success_body_venue_error))
            .route("/api/v1/recentTrades", get(handle_success_body_rate_limit)),
    )
    .await;
    let client = raw_client(base_url);
    let order_books_query = LighterOrderBooksQueryBuilder::default().build().unwrap();
    let recent_trades_query = LighterRecentTradesQuery {
        market_id: 0,
        limit: 10,
    };

    let venue_error = client
        .get_order_books(&order_books_query)
        .await
        .unwrap_err();
    let rate_limit_error = client
        .get_recent_trades(&recent_trades_query)
        .await
        .unwrap_err();

    assert!(matches!(
        venue_error,
        LighterHttpError::Venue { code: 1001, message } if message == "invalid market"
    ));
    assert!(matches!(
        rate_limit_error,
        LighterHttpError::RateLimit(message) if message == "slow down"
    ));
}

#[tokio::test]
async fn raw_client_retries_transient_5xx_then_succeeds() {
    let calls = Arc::new(AtomicUsize::new(0));
    let state = TransientFailureState {
        calls: calls.clone(),
        fail_until: 2,
        fail_status: StatusCode::SERVICE_UNAVAILABLE,
        success_body: HTTP_ORDER_BOOKS,
    };
    let base_url = spawn_server(
        Router::new()
            .route("/api/v1/orderBooks", get(handle_transient_failure))
            .with_state(state),
    )
    .await;
    let client = raw_client(base_url);
    let query = LighterOrderBooksQueryBuilder::default().build().unwrap();

    let response = client.get_order_books(&query).await.unwrap();

    assert_eq!(response.code, 200);
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn raw_client_retries_429_rate_limit_then_succeeds() {
    let calls = Arc::new(AtomicUsize::new(0));
    let state = TransientFailureState {
        calls: calls.clone(),
        fail_until: 1,
        fail_status: StatusCode::TOO_MANY_REQUESTS,
        success_body: HTTP_ORDER_BOOKS,
    };
    let base_url = spawn_server(
        Router::new()
            .route("/api/v1/orderBooks", get(handle_transient_failure))
            .with_state(state),
    )
    .await;
    let client = raw_client(base_url);
    let query = LighterOrderBooksQueryBuilder::default().build().unwrap();

    let response = client.get_order_books(&query).await.unwrap();

    assert_eq!(response.code, 200);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn raw_client_does_not_retry_4xx_other_than_429() {
    let calls = Arc::new(AtomicUsize::new(0));
    let state = TransientFailureState {
        calls: calls.clone(),
        fail_until: u32::MAX,
        fail_status: StatusCode::BAD_REQUEST,
        success_body: HTTP_ORDER_BOOKS,
    };
    let base_url = spawn_server(
        Router::new()
            .route("/api/v1/orderBooks", get(handle_transient_failure))
            .with_state(state),
    )
    .await;
    let client = raw_client(base_url);
    let query = LighterOrderBooksQueryBuilder::default().build().unwrap();

    let error = client.get_order_books(&query).await.unwrap_err();

    assert!(matches!(error, LighterHttpError::Http { status: 400, .. }));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn raw_client_retries_5xx_carrying_structured_venue_body() {
    // Pins the status-first guard: a 5xx with a `{code,message}` body
    // must not collapse to non-retryable Venue.
    let calls = Arc::new(AtomicUsize::new(0));
    let state = StructuredFailureState {
        calls: calls.clone(),
        fail_until: 2,
        fail_status: StatusCode::SERVICE_UNAVAILABLE,
        fail_body: r#"{"code":50001,"message":"server busy"}"#,
        success_body: HTTP_ORDER_BOOKS,
    };
    let base_url = spawn_server(
        Router::new()
            .route("/api/v1/orderBooks", get(handle_structured_failure))
            .with_state(state),
    )
    .await;
    let client = raw_client(base_url);
    let query = LighterOrderBooksQueryBuilder::default().build().unwrap();

    let response = client.get_order_books(&query).await.unwrap();

    assert_eq!(response.code, 200);
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn raw_client_retries_429_carrying_unrelated_venue_body() {
    // Same shape as the 5xx case: 429 must classify by status, not by a
    // non-429 venue code in the body.
    let calls = Arc::new(AtomicUsize::new(0));
    let state = StructuredFailureState {
        calls: calls.clone(),
        fail_until: 1,
        fail_status: StatusCode::TOO_MANY_REQUESTS,
        fail_body: r#"{"code":1001,"message":"slow down"}"#,
        success_body: HTTP_ORDER_BOOKS,
    };
    let base_url = spawn_server(
        Router::new()
            .route("/api/v1/orderBooks", get(handle_structured_failure))
            .with_state(state),
    )
    .await;
    let client = raw_client(base_url);
    let query = LighterOrderBooksQueryBuilder::default().build().unwrap();

    let response = client.get_order_books(&query).await.unwrap();

    assert_eq!(response.code, 200);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn raw_client_send_tx_post_is_single_shot_on_5xx() {
    // sendTx POST must not auto-retry: signed-nonce idempotency.
    let calls = Arc::new(AtomicUsize::new(0));
    let state = TransientFailureState {
        calls: calls.clone(),
        fail_until: u32::MAX,
        fail_status: StatusCode::SERVICE_UNAVAILABLE,
        success_body: "",
    };
    let base_url = spawn_server(
        Router::new()
            .route("/api/v1/sendTx", post(handle_transient_post))
            .with_state(state),
    )
    .await;
    let client = raw_client(base_url);
    let request = LighterSendTxRequest::new(LighterTxType::CreateOrder as u8, "{}".to_string());

    let error = client.send_tx(&request).await.unwrap_err();

    assert!(matches!(error, LighterHttpError::Http { status: 503, .. }));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn domain_client_registers_markets_and_parses_recent_trades() {
    let base_url = spawn_server(
        Router::new()
            .route("/api/v1/orderBookDetails", get(handle_order_book_details))
            .route("/api/v1/orderBookOrders", get(handle_order_book_orders))
            .route("/api/v1/recentTrades", get(handle_recent_trades)),
    )
    .await;
    let client =
        LighterHttpClient::new(LighterEnvironment::Mainnet, Some(base_url), 10, None).unwrap();
    let instrument = create_test_instrument();

    client
        .get_order_book_details(&LighterOrderBookDetailsQuery::default())
        .await
        .unwrap();
    let instruments = client.request_instruments().await.unwrap();
    let instruments_with_status = client.request_instruments_with_status().await.unwrap();
    let requested_instrument = client.request_instrument(instrument.id()).await.unwrap();
    let (requested_with_status, requested_status) = client
        .request_instrument_with_status(instrument.id())
        .await
        .unwrap();
    let ticks = client.request_recent_trades(&instrument, 10).await.unwrap();
    let deltas = client
        .request_order_book_snapshot(&instrument, 25)
        .await
        .unwrap();

    assert_eq!(client.market_registry().len(), 1);
    assert_eq!(instruments.len(), 1);
    assert_eq!(instruments[0].id(), instrument.id());
    assert_eq!(instruments_with_status.len(), 1);
    assert_eq!(instruments_with_status[0].0.id(), instrument.id());
    assert_eq!(instruments_with_status[0].1, LighterMarketStatus::Active);
    assert_eq!(requested_instrument.id(), instrument.id());
    assert_eq!(requested_with_status.id(), instrument.id());
    assert_eq!(requested_status, LighterMarketStatus::Active);

    match &instruments[0] {
        InstrumentAny::CryptoPerpetual(perp) => {
            assert_eq!(perp.raw_symbol.as_str(), "ETH");
            assert_eq!(perp.base_currency, Currency::from("ETH"));
            assert_eq!(perp.quote_currency, Currency::from("USDC"));
            assert_eq!(perp.settlement_currency, Currency::from("USDC"));
            assert_eq!(perp.price_increment, Price::from("0.01"));
            assert_eq!(perp.size_increment, Quantity::from("0.0001"));
            assert_eq!(perp.min_quantity, Some(Quantity::from("0.0050")));
        }
        other => panic!("expected crypto perpetual, was {other:?}"),
    }
    assert_eq!(ticks.len(), 1);
    assert_eq!(ticks[0].instrument_id, instrument.id());
    assert_eq!(ticks[0].price, Price::from("2361.31"));
    assert_eq!(ticks[0].size, Quantity::from("0.0005"));
    assert_eq!(deltas.instrument_id, instrument.id());
    assert_eq!(deltas.deltas.len(), 3);
    assert_eq!(deltas.deltas[0].action, BookAction::Clear);
    assert_eq!(deltas.deltas[0].sequence, 0);
    assert_eq!(deltas.deltas[1].action, BookAction::Add);
    assert_eq!(deltas.deltas[1].order.side, OrderSide::Buy);
    assert_eq!(deltas.deltas[1].order.price, Price::from("2361.17"));
    assert_eq!(deltas.deltas[1].order.size, Quantity::from("3.4125"));
    assert_eq!(deltas.deltas[1].sequence, 1);
    assert_eq!(deltas.deltas[2].action, BookAction::Add);
    assert_eq!(deltas.deltas[2].order.side, OrderSide::Sell);
    assert_eq!(deltas.deltas[2].order.price, Price::from("2361.32"));
    assert_eq!(deltas.deltas[2].order.size, Quantity::from("0.0317"));
    assert_eq!(deltas.deltas[2].sequence, 2);
    assert_eq!(
        deltas.deltas[2].flags & RecordFlag::F_LAST as u8,
        RecordFlag::F_LAST as u8
    );
}

#[tokio::test]
async fn domain_client_request_trades_fills_market_id_and_parses_ticks() {
    let base_url = spawn_server(
        Router::new()
            .route("/api/v1/orderBookDetails", get(handle_order_book_details))
            .route("/api/v1/trades", get(handle_domain_trades)),
    )
    .await;
    let client =
        LighterHttpClient::new(LighterEnvironment::Mainnet, Some(base_url), 10, None).unwrap();
    let instrument = create_test_instrument();

    client
        .get_order_book_details(&LighterOrderBookDetailsQuery::default())
        .await
        .unwrap();
    let ticks = client
        .request_trades(
            &instrument,
            LighterTradesQuery {
                limit: 50,
                ..Default::default()
            },
        )
        .await
        .unwrap();

    assert_eq!(ticks.len(), 1);
    assert_eq!(ticks[0].instrument_id, instrument.id());
    assert_eq!(ticks[0].price, Price::from("2361.31"));
    assert_eq!(ticks[0].size, Quantity::from("0.0005"));
    assert_eq!(ticks[0].aggressor_side, AggressorSide::Seller);
    assert_eq!(ticks[0].trade_id.to_string(), "19211490282");
}

#[tokio::test]
async fn domain_client_request_bars_fills_market_id_and_parses_bars() {
    let base_url = spawn_server(
        Router::new()
            .route("/api/v1/orderBookDetails", get(handle_order_book_details))
            .route("/api/v1/candles", get(handle_domain_candles)),
    )
    .await;
    let client =
        LighterHttpClient::new(LighterEnvironment::Mainnet, Some(base_url), 10, None).unwrap();
    let instrument = create_test_instrument();
    let bar_type = BarType::new(
        instrument.id(),
        BarSpecification::new(1, BarAggregation::Minute, PriceType::Last),
        AggregationSource::External,
    );
    let start = Utc.timestamp_millis_opt(1_700_000_000_000).unwrap();
    let end = Utc.timestamp_millis_opt(1_700_000_120_000).unwrap();

    client
        .get_order_book_details(&LighterOrderBookDetailsQuery::default())
        .await
        .unwrap();
    let bars = client
        .request_bars(&instrument, bar_type, Some(start), Some(end), Some(2))
        .await
        .unwrap();

    assert_eq!(bars.len(), 2);
    assert_eq!(bars[0].bar_type, bar_type);
    assert_eq!(bars[0].open, Price::from("2361.11"));
    assert_eq!(bars[0].high, Price::from("2362.22"));
    assert_eq!(bars[0].low, Price::from("2360.00"));
    assert_eq!(bars[0].close, Price::from("2361.31"));
    assert_eq!(bars[0].volume, Quantity::from("1.2345"));
    assert_eq!(bars[0].ts_event, UnixNanos::from(1_700_000_000_000_000_000));
}

#[tokio::test]
async fn domain_client_request_funding_rates_parses_signed_rates() {
    let base_url = spawn_server(
        Router::new()
            .route("/api/v1/orderBookDetails", get(handle_order_book_details))
            .route("/api/v1/fundings", get(handle_domain_fundings)),
    )
    .await;
    let client =
        LighterHttpClient::new(LighterEnvironment::Mainnet, Some(base_url), 10, None).unwrap();
    let instrument = create_test_instrument();
    let start = Utc
        .timestamp_millis_opt(1_778_702_400_000)
        .single()
        .unwrap();
    let end = Utc
        .timestamp_millis_opt(1_778_706_000_000)
        .single()
        .unwrap();

    client
        .get_order_book_details(&LighterOrderBookDetailsQuery::default())
        .await
        .unwrap();
    let funding_rates = client
        .request_funding_rates(&instrument, Some(start), Some(end), Some(2))
        .await
        .unwrap();

    assert_eq!(funding_rates.len(), 2);
    assert_eq!(funding_rates[0].instrument_id, instrument.id());
    assert_eq!(funding_rates[0].rate, Decimal::new(12, 4));
    assert_eq!(funding_rates[0].interval, Some(60));
    assert_eq!(
        funding_rates[0].ts_event,
        UnixNanos::from(1_778_702_400_000_000_000),
    );
    assert_eq!(funding_rates[1].rate, Decimal::new(-2, 4));
}

#[tokio::test]
async fn domain_client_request_funding_rates_paginates_range() {
    let calls = Arc::new(AtomicUsize::new(0));
    let interval_ms = LighterFundingResolution::OneHour.interval_millis();
    let state = PaginatedFundingsState {
        start_ms: 1_778_702_400_000,
        calls: Arc::clone(&calls),
    };
    let page_span_ms = i64::from(LIGHTER_FUNDINGS_MAX_LIMIT - 1) * interval_ms;
    let boundary_ms = state.start_ms + page_span_ms;
    let end_ms = boundary_ms + interval_ms;
    let base_url = spawn_server(
        Router::new()
            .route("/api/v1/orderBookDetails", get(handle_order_book_details))
            .route("/api/v1/fundings", get(handle_paginated_fundings))
            .with_state(state.clone()),
    )
    .await;
    let client =
        LighterHttpClient::new(LighterEnvironment::Mainnet, Some(base_url), 10, None).unwrap();
    let instrument = create_test_instrument();
    let start = Utc.timestamp_millis_opt(state.start_ms).single().unwrap();
    let end = Utc.timestamp_millis_opt(end_ms).single().unwrap();

    client
        .get_order_book_details(&LighterOrderBookDetailsQuery::default())
        .await
        .unwrap();
    let funding_rates = client
        .request_funding_rates(&instrument, Some(start), Some(end), None)
        .await
        .unwrap();

    // Two pages cover the range; the row on the page boundary is stitched once.
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(funding_rates.len(), 3);
    assert_eq!(
        funding_rates[0].ts_event,
        millis_to_unix_nanos(state.start_ms)
    );
    assert_eq!(funding_rates[0].rate, Decimal::new(12, 4));
    assert_eq!(funding_rates[1].ts_event, millis_to_unix_nanos(boundary_ms));
    assert_eq!(funding_rates[1].rate, Decimal::new(-2, 4));
    assert_eq!(funding_rates[2].ts_event, millis_to_unix_nanos(end_ms));
    assert_eq!(funding_rates[2].rate, Decimal::new(1, 4));
}

#[tokio::test]
async fn domain_client_request_funding_rates_caps_to_limit_across_pages() {
    let calls = Arc::new(AtomicUsize::new(0));
    let interval_ms = LighterFundingResolution::OneHour.interval_millis();
    let state = PaginatedFundingsState {
        start_ms: 1_778_702_400_000,
        calls: Arc::clone(&calls),
    };
    let page_span_ms = i64::from(LIGHTER_FUNDINGS_MAX_LIMIT - 1) * interval_ms;
    let end_ms = state.start_ms + page_span_ms + interval_ms;
    let base_url = spawn_server(
        Router::new()
            .route("/api/v1/orderBookDetails", get(handle_order_book_details))
            .route("/api/v1/fundings", get(handle_limit_paginated_fundings))
            .with_state(state.clone()),
    )
    .await;
    let client =
        LighterHttpClient::new(LighterEnvironment::Mainnet, Some(base_url), 10, None).unwrap();
    let instrument = create_test_instrument();
    let start = Utc.timestamp_millis_opt(state.start_ms).single().unwrap();
    let end = Utc.timestamp_millis_opt(end_ms).single().unwrap();

    client
        .get_order_book_details(&LighterOrderBookDetailsQuery::default())
        .await
        .unwrap();
    let funding_rates = client
        .request_funding_rates(&instrument, Some(start), Some(end), Some(2))
        .await
        .unwrap();

    // The limit is reached inside the first page, so the second page is never fetched
    // and the rows beyond the limit are dropped.
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(funding_rates.len(), 2);
    assert_eq!(
        funding_rates[0].ts_event,
        millis_to_unix_nanos(state.start_ms)
    );
    assert_eq!(funding_rates[0].rate, Decimal::new(12, 4));
    assert_eq!(
        funding_rates[1].ts_event,
        millis_to_unix_nanos(state.start_ms + interval_ms)
    );
    assert_eq!(funding_rates[1].rate, Decimal::new(3, 4));
}

#[tokio::test]
async fn domain_client_request_funding_rates_without_start_returns_latest_limit() {
    let interval_ms = LighterFundingResolution::OneHour.interval_millis();
    let state = LatestFundingsState {
        end_ms: 1_778_706_000_000,
    };
    let base_url = spawn_server(
        Router::new()
            .route("/api/v1/orderBookDetails", get(handle_order_book_details))
            .route("/api/v1/fundings", get(handle_latest_fundings))
            .with_state(state.clone()),
    )
    .await;
    let client =
        LighterHttpClient::new(LighterEnvironment::Mainnet, Some(base_url), 10, None).unwrap();
    let instrument = create_test_instrument();
    let end = Utc.timestamp_millis_opt(state.end_ms).single().unwrap();

    client
        .get_order_book_details(&LighterOrderBookDetailsQuery::default())
        .await
        .unwrap();
    let funding_rates = client
        .request_funding_rates(&instrument, None, Some(end), Some(2))
        .await
        .unwrap();

    // The latest two settled rows are kept when no start is given.
    assert_eq!(funding_rates.len(), 2);
    assert_eq!(
        funding_rates[0].ts_event,
        millis_to_unix_nanos(state.end_ms - interval_ms)
    );
    assert_eq!(funding_rates[0].rate, Decimal::new(7, 4));
    assert_eq!(
        funding_rates[1].ts_event,
        millis_to_unix_nanos(state.end_ms)
    );
    assert_eq!(funding_rates[1].rate, Decimal::new(-8, 4));
}

#[tokio::test]
async fn domain_client_request_bars_filters_incomplete_candle() {
    let now_ms = Utc::now().timestamp_millis();
    let state = IncompleteCandlesState {
        completed_start_ms: now_ms - 2 * MINUTE_MS,
        incomplete_start_ms: now_ms - MINUTE_MS / 2,
    };
    let base_url = spawn_server(
        Router::new()
            .route("/api/v1/orderBookDetails", get(handle_order_book_details))
            .route("/api/v1/candles", get(handle_incomplete_candles))
            .with_state(state.clone()),
    )
    .await;
    let client =
        LighterHttpClient::new(LighterEnvironment::Mainnet, Some(base_url), 10, None).unwrap();
    let instrument = create_test_instrument();
    let bar_type = one_minute_bar_type(instrument.id());
    let start = Utc
        .timestamp_millis_opt(state.completed_start_ms)
        .single()
        .unwrap();
    let end = Utc
        .timestamp_millis_opt(now_ms + MINUTE_MS)
        .single()
        .unwrap();

    client
        .get_order_book_details(&LighterOrderBookDetailsQuery::default())
        .await
        .unwrap();
    let bars = client
        .request_bars(&instrument, bar_type, Some(start), Some(end), Some(2))
        .await
        .unwrap();

    assert_eq!(bars.len(), 1);
    assert_eq!(
        bars[0].ts_event,
        millis_to_unix_nanos(state.completed_start_ms),
    );
    assert_eq!(bars[0].close, Price::from("10.25"));
}

#[tokio::test]
async fn domain_client_request_bars_without_start_returns_latest_completed_limit() {
    let state = LatestCandlesState {
        end_ms: 1_700_000_180_000,
    };
    let base_url = spawn_server(
        Router::new()
            .route("/api/v1/orderBookDetails", get(handle_order_book_details))
            .route("/api/v1/candles", get(handle_latest_candles))
            .with_state(state.clone()),
    )
    .await;
    let client =
        LighterHttpClient::new(LighterEnvironment::Mainnet, Some(base_url), 10, None).unwrap();
    let instrument = create_test_instrument();
    let bar_type = one_minute_bar_type(instrument.id());
    let end = Utc.timestamp_millis_opt(state.end_ms).single().unwrap();

    client
        .get_order_book_details(&LighterOrderBookDetailsQuery::default())
        .await
        .unwrap();
    let bars = client
        .request_bars(&instrument, bar_type, None, Some(end), Some(2))
        .await
        .unwrap();

    assert_eq!(bars.len(), 2);
    assert_eq!(
        bars[0].ts_event,
        millis_to_unix_nanos(state.end_ms - 2 * MINUTE_MS)
    );
    assert_eq!(bars[0].close, Price::from("11.25"));
    assert_eq!(
        bars[1].ts_event,
        millis_to_unix_nanos(state.end_ms - MINUTE_MS)
    );
    assert_eq!(bars[1].close, Price::from("12.25"));
}

#[tokio::test]
async fn domain_client_request_bars_paginates_range() {
    let calls = Arc::new(AtomicUsize::new(0));
    let state = PaginatedCandlesState {
        start_ms: 1_700_000_000_000,
        calls: Arc::clone(&calls),
    };
    let end_ms = state.start_ms + (i64::from(LIGHTER_CANDLES_MAX_LIMIT) + 1) * MINUTE_MS;
    let base_url = spawn_server(
        Router::new()
            .route("/api/v1/orderBookDetails", get(handle_order_book_details))
            .route("/api/v1/candles", get(handle_paginated_candles))
            .with_state(state.clone()),
    )
    .await;
    let client =
        LighterHttpClient::new(LighterEnvironment::Mainnet, Some(base_url), 10, None).unwrap();
    let instrument = create_test_instrument();
    let bar_type = one_minute_bar_type(instrument.id());
    let start = Utc.timestamp_millis_opt(state.start_ms).single().unwrap();
    let end = Utc.timestamp_millis_opt(end_ms).single().unwrap();

    client
        .get_order_book_details(&LighterOrderBookDetailsQuery::default())
        .await
        .unwrap();
    let bars = client
        .request_bars(&instrument, bar_type, Some(start), Some(end), None)
        .await
        .unwrap();

    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(bars.len(), 2);
    assert_eq!(bars[0].ts_event, millis_to_unix_nanos(state.start_ms));
    assert_eq!(
        bars[1].ts_event,
        millis_to_unix_nanos(state.start_ms + i64::from(LIGHTER_CANDLES_MAX_LIMIT) * MINUTE_MS),
    );
}

#[tokio::test]
async fn domain_client_request_bars_rejects_unsupported_bar_type() {
    let base_url = spawn_server(
        Router::new().route("/api/v1/orderBookDetails", get(handle_order_book_details)),
    )
    .await;
    let client =
        LighterHttpClient::new(LighterEnvironment::Mainnet, Some(base_url), 10, None).unwrap();
    let instrument = create_test_instrument();
    let bar_type = unsupported_three_minute_bar_type(instrument.id());
    let start = Utc
        .timestamp_millis_opt(1_700_000_000_000)
        .single()
        .unwrap();
    let end = Utc
        .timestamp_millis_opt(1_700_000_060_000)
        .single()
        .unwrap();

    client
        .get_order_book_details(&LighterOrderBookDetailsQuery::default())
        .await
        .unwrap();
    let error = client
        .request_bars(&instrument, bar_type, Some(start), Some(end), Some(1))
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        LighterHttpError::Parse(message)
            if message == "unsupported Lighter candle minute step: 3"
    ));
}

#[tokio::test]
async fn domain_client_request_instrument_errors_when_not_found() {
    let base_url = spawn_server(
        Router::new().route("/api/v1/orderBookDetails", get(handle_order_book_details)),
    )
    .await;
    let client =
        LighterHttpClient::new(LighterEnvironment::Mainnet, Some(base_url), 10, None).unwrap();
    let instrument_id = InstrumentId::new(Symbol::new("BTC-PERP"), Venue::new("LIGHTER"));

    let error = client.request_instrument(instrument_id).await.unwrap_err();

    assert!(matches!(
        error,
        LighterHttpError::Parse(message)
            if message == "instrument BTC-PERP.LIGHTER not found"
    ));
}

#[tokio::test]
async fn domain_client_get_account_detail_queries_by_index_and_parses_first_account() {
    let base_url = spawn_server(Router::new().route("/api/v1/account", get(handle_account))).await;
    let client =
        LighterHttpClient::new(LighterEnvironment::Mainnet, Some(base_url), 10, None).unwrap();

    let detail = client.get_account_detail(123_456).await.unwrap();

    assert_eq!(detail.account_index, 123_456);
    assert_eq!(detail.account_type, 0);
    assert_eq!(detail.status, 1);
}

#[tokio::test]
async fn domain_client_get_account_detail_errors_on_empty_accounts() {
    let base_url =
        spawn_server(Router::new().route("/api/v1/account", get(handle_account_empty))).await;
    let client =
        LighterHttpClient::new(LighterEnvironment::Mainnet, Some(base_url), 10, None).unwrap();

    let error = client.get_account_detail(123_456).await.unwrap_err();

    assert!(matches!(
        error,
        LighterHttpError::Parse(message)
            if message == "no account returned for index 123456"
    ));
}

async fn handle_next_nonce(Query(query): Query<LighterNextNonceQuery>) -> Response {
    assert_eq!(query.account_index, 12_345);
    assert_eq!(query.api_key_index, 5);
    (StatusCode::OK, HTTP_NEXT_NONCE).into_response()
}

async fn handle_maker_only_api_keys(
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let authorization = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok());

    if authorization != Some("auth-token")
        || query.get("account_index").map(String::as_str) != Some("712440")
        || query.contains_key("auth")
        || query.contains_key("authorization")
    {
        return (
            StatusCode::BAD_REQUEST,
            r#"{"code":400,"message":"unexpected maker-only request"}"#,
        )
            .into_response();
    }

    (StatusCode::OK, r#"{"code":200,"api_key_indexes":[5]}"#).into_response()
}

async fn handle_send_tx(headers: HeaderMap, body: Bytes) -> Response {
    let body = assert_lighter_multipart_body(&headers, &body);
    assert!(body.contains("name=\"tx_type\"\r\n\r\n14\r\n"));
    assert!(body.contains(
        "name=\"tx_info\"\r\n\r\n{\"AccountIndex\":1,\"Nonce\":2,\"Sig\":\"0xsig\"}\r\n"
    ));
    assert!(body.contains("name=\"price_protection\"\r\n\r\ntrue\r\n"));
    (
        StatusCode::OK,
        r#"{"code":200,"tx_hash":"0xabc","predicted_execution_time_ms":1751465474,"volume_quota_remaining":123}"#,
    )
        .into_response()
}

async fn handle_send_tx_false_price_protection(headers: HeaderMap, body: Bytes) -> Response {
    let body = assert_lighter_multipart_body(&headers, &body);
    assert!(body.contains("name=\"tx_type\"\r\n\r\n14\r\n"));
    assert!(body.contains(
        "name=\"tx_info\"\r\n\r\n{\"AccountIndex\":1,\"Nonce\":2,\"Sig\":\"0xsig\"}\r\n"
    ));
    assert!(body.contains("name=\"price_protection\"\r\n\r\nfalse\r\n"));
    (
        StatusCode::OK,
        r#"{"code":200,"tx_hash":"0xabc","predicted_execution_time_ms":1751465474,"volume_quota_remaining":123}"#,
    )
        .into_response()
}

async fn handle_send_tx_batch(headers: HeaderMap, body: Bytes) -> Response {
    let body = assert_lighter_multipart_body(&headers, &body);
    assert!(body.contains("name=\"tx_types\"\r\n\r\n[14,15]\r\n"));
    assert!(
        body.contains("name=\"tx_infos\"\r\n\r\n[{\"AccountIndex\":1},{\"AccountIndex\":1}]\r\n")
    );
    (
        StatusCode::OK,
        concat!(
            r#"{"code":200,"tx_hash":["0xabc","0xdef"],"#,
            r#""predicted_execution_time_ms":1751465475,"volume_quota_remaining":122}"#,
        ),
    )
        .into_response()
}

async fn handle_send_tx_batch_without_volume_quota(headers: HeaderMap, body: Bytes) -> Response {
    let body = assert_lighter_multipart_body(&headers, &body);
    assert!(body.contains("name=\"tx_types\"\r\n\r\n[14,15]\r\n"));
    (
        StatusCode::OK,
        r#"{"code":200,"tx_hash":["0xabc","0xdef"],"predicted_execution_time_ms":1751465475}"#,
    )
        .into_response()
}

async fn handle_send_tx_success_body_venue_error(headers: HeaderMap, body: Bytes) -> Response {
    let body = assert_lighter_multipart_body(&headers, &body);
    assert!(body.contains("name=\"tx_type\"\r\n\r\n14\r\n"));
    assert!(body.contains("name=\"tx_info\"\r\n\r\n"));
    assert!(!body.contains("name=\"price_protection\""));
    (StatusCode::OK, r#"{"code":1001,"message":"invalid tx"}"#).into_response()
}

async fn handle_send_tx_batch_success_body_rate_limit(headers: HeaderMap, body: Bytes) -> Response {
    let body = assert_lighter_multipart_body(&headers, &body);
    assert!(body.contains("name=\"tx_types\"\r\n\r\n[14,15]\r\n"));
    assert!(body.contains("name=\"tx_infos\"\r\n\r\n[]\r\n"));
    (StatusCode::OK, r#"{"code":429,"message":"slow down"}"#).into_response()
}

async fn handle_order_books(Query(query): Query<LighterOrderBooksQuery>) -> Response {
    assert_eq!(query.market_id, Some(0));
    assert_eq!(query.filter, Some(LighterOrderBookFilter::Perp));
    (StatusCode::OK, HTTP_ORDER_BOOKS).into_response()
}

async fn handle_order_book_details_filtered(
    Query(query): Query<LighterOrderBookDetailsQuery>,
) -> Response {
    assert_eq!(query.market_id, Some(0));
    assert_eq!(query.filter, Some(LighterOrderBookFilter::Perp));
    (StatusCode::OK, HTTP_ORDER_BOOK_DETAILS).into_response()
}

async fn handle_order_book_details() -> Response {
    (StatusCode::OK, HTTP_ORDER_BOOK_DETAILS).into_response()
}

async fn handle_order_book_orders(Query(query): Query<LighterOrderBookOrdersQuery>) -> Response {
    assert_eq!(query.market_id, 0);
    assert_eq!(query.limit, 25);
    (StatusCode::OK, HTTP_ORDER_BOOK_ORDERS).into_response()
}

async fn handle_recent_trades(Query(query): Query<LighterRecentTradesQuery>) -> Response {
    assert_eq!(query.market_id, 0);
    assert_eq!(query.limit, 10);
    (StatusCode::OK, HTTP_RECENT_TRADES).into_response()
}

async fn handle_domain_trades(Query(query): Query<LighterTradesQuery>) -> Response {
    assert_eq!(query.authorization, None);
    assert_eq!(query.auth, None);
    assert_eq!(query.market_id, Some(0));
    assert_eq!(query.account_index, None);
    assert_eq!(query.order_index, None);
    assert_eq!(query.sort_by, LighterTradeSortBy::TradeId);
    assert_eq!(query.sort_dir, None);
    assert_eq!(query.cursor, None);
    assert_eq!(query.from_timestamp, None);
    assert_eq!(query.ask_filter, None);
    assert_eq!(query.role, None);
    assert_eq!(query.trade_type, None);
    assert_eq!(query.limit, 50);
    assert_eq!(query.aggregate, None);
    (StatusCode::OK, HTTP_RECENT_TRADES).into_response()
}

async fn handle_candles(Query(query): Query<LighterCandlesQuery>) -> Response {
    assert_eq!(query.market_id, 0);
    assert_eq!(query.resolution, LighterCandleResolution::OneMinute);
    assert_eq!(query.start_timestamp, 1_700_000_000_000);
    assert_eq!(query.end_timestamp, 1_700_000_120_000);
    assert_eq!(query.count_back, i64::from(LIGHTER_CANDLES_MAX_LIMIT));
    assert_eq!(query.set_timestamp_to_end, Some(false));
    (StatusCode::OK, HTTP_CANDLES).into_response()
}

async fn handle_fundings(Query(query): Query<LighterFundingsQuery>) -> Response {
    assert_eq!(query.market_id, 0);
    assert_eq!(query.resolution, LighterFundingResolution::OneHour);
    assert_eq!(query.start_timestamp, 1_778_702_400_000);
    assert_eq!(query.end_timestamp, 1_778_706_000_000);
    assert_eq!(query.count_back, 2);
    (StatusCode::OK, HTTP_FUNDINGS).into_response()
}

async fn handle_domain_fundings(Query(query): Query<LighterFundingsQuery>) -> Response {
    assert_eq!(query.market_id, 0);
    assert_eq!(query.resolution, LighterFundingResolution::OneHour);
    assert_eq!(query.start_timestamp, 1_778_702_400_000);
    assert_eq!(query.end_timestamp, 1_778_706_000_000);
    assert_eq!(query.count_back, i64::from(LIGHTER_FUNDINGS_MAX_LIMIT));
    (StatusCode::OK, HTTP_FUNDINGS).into_response()
}

async fn handle_paginated_fundings(
    State(state): State<PaginatedFundingsState>,
    Query(query): Query<LighterFundingsQuery>,
) -> Response {
    assert_eq!(query.market_id, 0);
    assert_eq!(query.resolution, LighterFundingResolution::OneHour);
    assert_eq!(query.count_back, i64::from(LIGHTER_FUNDINGS_MAX_LIMIT));
    let page = state.calls.fetch_add(1, Ordering::SeqCst);
    let interval_ms = LighterFundingResolution::OneHour.interval_millis();
    let page_span_ms = i64::from(LIGHTER_FUNDINGS_MAX_LIMIT - 1) * interval_ms;
    // Each window spans at most `cap - 1` intervals so `count_back == cap` keeps
    // the window's first row (the endpoint excludes end_timestamp).
    assert!(query.end_timestamp - query.start_timestamp <= page_span_ms);
    let boundary_ms = state.start_ms + page_span_ms;
    let end_ms = boundary_ms + interval_ms;

    match page {
        0 => {
            assert_eq!(query.start_timestamp, state.start_ms);
            assert_eq!(query.end_timestamp, boundary_ms);
            let body = fundings_response(&[
                (state.start_ms / 1000, "0.0012", "long"),
                (boundary_ms / 1000, "0.0002", "short"),
            ]);
            (StatusCode::OK, body).into_response()
        }
        1 => {
            assert_eq!(query.start_timestamp, boundary_ms);
            assert_eq!(query.end_timestamp, end_ms);
            let body = fundings_response(&[
                (boundary_ms / 1000, "0.0002", "short"),
                (end_ms / 1000, "0.0001", "long"),
            ]);
            (StatusCode::OK, body).into_response()
        }
        _ => (StatusCode::INTERNAL_SERVER_ERROR, "unexpected funding page").into_response(),
    }
}

async fn handle_limit_paginated_fundings(
    State(state): State<PaginatedFundingsState>,
    Query(query): Query<LighterFundingsQuery>,
) -> Response {
    assert_eq!(query.market_id, 0);
    assert_eq!(query.resolution, LighterFundingResolution::OneHour);
    assert_eq!(query.count_back, i64::from(LIGHTER_FUNDINGS_MAX_LIMIT));
    let page = state.calls.fetch_add(1, Ordering::SeqCst);
    let interval_ms = LighterFundingResolution::OneHour.interval_millis();
    let page_span_ms = i64::from(LIGHTER_FUNDINGS_MAX_LIMIT - 1) * interval_ms;
    let boundary_ms = state.start_ms + page_span_ms;
    let end_ms = boundary_ms + interval_ms;

    match page {
        0 => {
            assert_eq!(query.start_timestamp, state.start_ms);
            assert_eq!(query.end_timestamp, boundary_ms);
            let body = fundings_response(&[
                (state.start_ms / 1000, "0.0012", "long"),
                ((state.start_ms + interval_ms) / 1000, "0.0003", "long"),
                (boundary_ms / 1000, "0.0002", "short"),
            ]);
            (StatusCode::OK, body).into_response()
        }
        1 => {
            assert_eq!(query.start_timestamp, boundary_ms);
            assert_eq!(query.end_timestamp, end_ms);
            let body = fundings_response(&[
                (boundary_ms / 1000, "0.0002", "short"),
                (end_ms / 1000, "0.0001", "long"),
            ]);
            (StatusCode::OK, body).into_response()
        }
        _ => (StatusCode::INTERNAL_SERVER_ERROR, "unexpected funding page").into_response(),
    }
}

async fn handle_latest_fundings(
    State(state): State<LatestFundingsState>,
    Query(query): Query<LighterFundingsQuery>,
) -> Response {
    assert_eq!(query.market_id, 0);
    assert_eq!(query.resolution, LighterFundingResolution::OneHour);
    assert_eq!(query.count_back, i64::from(LIGHTER_FUNDINGS_MAX_LIMIT));
    let interval_ms = LighterFundingResolution::OneHour.interval_millis();
    // No start: the lookback spans `limit + 1` intervals (limit is 2 in this test).
    assert_eq!(query.start_timestamp, state.end_ms - 3 * interval_ms);
    assert_eq!(query.end_timestamp, state.end_ms);
    let body = fundings_response(&[
        ((state.end_ms - 3 * interval_ms) / 1000, "0.0005", "long"),
        ((state.end_ms - 2 * interval_ms) / 1000, "0.0006", "long"),
        ((state.end_ms - interval_ms) / 1000, "0.0007", "long"),
        (state.end_ms / 1000, "0.0008", "short"),
    ]);
    (StatusCode::OK, body).into_response()
}

async fn handle_domain_candles(Query(query): Query<LighterCandlesQuery>) -> Response {
    assert_eq!(query.market_id, 0);
    assert_eq!(query.resolution, LighterCandleResolution::OneMinute);
    assert_eq!(query.start_timestamp, 1_700_000_000_000);
    assert_eq!(query.end_timestamp, 1_700_000_120_000);
    assert_eq!(query.count_back, i64::from(LIGHTER_CANDLES_MAX_LIMIT));
    assert_eq!(query.set_timestamp_to_end, Some(false));
    (StatusCode::OK, HTTP_CANDLES).into_response()
}

async fn handle_incomplete_candles(
    State(state): State<IncompleteCandlesState>,
    Query(query): Query<LighterCandlesQuery>,
) -> Response {
    assert_candles_query_common(&query);
    assert_eq!(query.start_timestamp, state.completed_start_ms);
    assert!(query.end_timestamp > state.incomplete_start_ms);
    let body = candles_response(&[
        (state.completed_start_ms, "10.25"),
        (state.incomplete_start_ms, "99.99"),
    ]);
    (StatusCode::OK, body).into_response()
}

async fn handle_latest_candles(
    State(state): State<LatestCandlesState>,
    Query(query): Query<LighterCandlesQuery>,
) -> Response {
    assert_candles_query_common(&query);
    assert_eq!(query.start_timestamp, state.end_ms - 3 * MINUTE_MS);
    assert_eq!(query.end_timestamp, state.end_ms);
    let body = candles_response(&[
        (state.end_ms - 3 * MINUTE_MS, "10.25"),
        (state.end_ms - 2 * MINUTE_MS, "11.25"),
        (state.end_ms - MINUTE_MS, "12.25"),
    ]);
    (StatusCode::OK, body).into_response()
}

async fn handle_paginated_candles(
    State(state): State<PaginatedCandlesState>,
    Query(query): Query<LighterCandlesQuery>,
) -> Response {
    assert_candles_query_common(&query);
    let page = state.calls.fetch_add(1, Ordering::SeqCst);
    let page_span_ms = i64::from(LIGHTER_CANDLES_MAX_LIMIT) * MINUTE_MS;

    match page {
        0 => {
            assert_eq!(query.start_timestamp, state.start_ms);
            assert_eq!(query.end_timestamp, state.start_ms + page_span_ms);
            let body = candles_response(&[(state.start_ms, "10.25")]);
            (StatusCode::OK, body).into_response()
        }
        1 => {
            assert_eq!(query.start_timestamp, state.start_ms + page_span_ms);
            assert_eq!(
                query.end_timestamp,
                state.start_ms + page_span_ms + MINUTE_MS
            );
            let body = candles_response(&[
                (state.start_ms, "99.99"),
                (state.start_ms + page_span_ms, "11.25"),
            ]);
            (StatusCode::OK, body).into_response()
        }
        _ => (StatusCode::INTERNAL_SERVER_ERROR, "unexpected candle page").into_response(),
    }
}

async fn handle_trades(Query(query): Query<LighterTradesQuery>) -> Response {
    assert_eq!(query.authorization.as_deref(), Some("bearer-token"));
    assert_eq!(query.market_id, Some(0));
    assert_eq!(query.account_index, Some(712_440));
    assert_eq!(query.order_index, Some(281_476_929_510_110));
    assert_eq!(query.sort_by, LighterTradeSortBy::Timestamp);
    assert_eq!(query.sort_dir, Some(LighterSortDirection::Desc));
    assert_eq!(query.cursor.as_deref(), Some("cursor-1"));
    assert_eq!(query.from_timestamp, Some(1_700_000_000_000));
    assert_eq!(query.ask_filter, Some(1));
    assert_eq!(query.role, Some(LighterTradeRole::Maker));
    assert_eq!(query.trade_type, Some(LighterTradeQueryType::Trade));
    assert_eq!(query.limit, 50);
    assert_eq!(query.aggregate, Some(true));
    (StatusCode::OK, HTTP_RECENT_TRADES).into_response()
}

async fn handle_trades_with_page_size_limit(Query(query): Query<LighterTradesQuery>) -> Response {
    assert_eq!(query.limit, LIGHTER_REST_PAGE_SIZE);
    (StatusCode::OK, HTTP_RECENT_TRADES).into_response()
}

async fn handle_account_active_orders(
    Query(query): Query<LighterAccountActiveOrdersQuery>,
) -> Response {
    assert_eq!(query.auth.as_deref(), Some("auth-token"));
    assert_eq!(query.account_index, 712_440);
    assert_eq!(query.market_id, 0);
    (StatusCode::OK, HTTP_ORDERS).into_response()
}

async fn handle_account_inactive_orders(
    Query(query): Query<LighterAccountInactiveOrdersQuery>,
) -> Response {
    assert_eq!(query.authorization.as_deref(), Some("bearer-token"));
    assert_eq!(query.account_index, 712_440);
    assert_eq!(query.market_id, Some(0));
    assert_eq!(query.ask_filter, Some(1));
    assert_eq!(
        query.between_timestamps.as_deref(),
        Some("1700000000000,1700000001000"),
    );
    assert_eq!(query.cursor.as_deref(), Some("cursor-1"));
    assert_eq!(query.limit, 50);
    (StatusCode::OK, HTTP_ORDERS).into_response()
}

async fn handle_account(Query(query): Query<LighterAccountQuery>) -> Response {
    assert_eq!(query.by, LighterAccountLookup::Index);
    assert_eq!(query.value, "123456");
    (StatusCode::OK, HTTP_ACCOUNT).into_response()
}

async fn handle_account_empty() -> Response {
    (StatusCode::OK, r#"{"code":200,"total":0,"accounts":[]}"#).into_response()
}

async fn handle_rate_limit() -> Response {
    (StatusCode::TOO_MANY_REQUESTS, "too many requests").into_response()
}

#[derive(Clone)]
struct TransientFailureState {
    calls: Arc<AtomicUsize>,
    fail_until: u32,
    fail_status: StatusCode,
    success_body: &'static str,
}

async fn handle_transient_failure(State(state): State<TransientFailureState>) -> Response {
    let call = state.calls.fetch_add(1, Ordering::SeqCst) as u32;
    if call < state.fail_until {
        (state.fail_status, "").into_response()
    } else {
        (StatusCode::OK, state.success_body).into_response()
    }
}

async fn handle_transient_post(State(state): State<TransientFailureState>) -> Response {
    let call = state.calls.fetch_add(1, Ordering::SeqCst) as u32;
    if call < state.fail_until {
        (state.fail_status, "").into_response()
    } else {
        (StatusCode::OK, state.success_body).into_response()
    }
}

#[derive(Clone)]
struct StructuredFailureState {
    calls: Arc<AtomicUsize>,
    fail_until: u32,
    fail_status: StatusCode,
    fail_body: &'static str,
    success_body: &'static str,
}

async fn handle_structured_failure(State(state): State<StructuredFailureState>) -> Response {
    let call = state.calls.fetch_add(1, Ordering::SeqCst) as u32;
    if call < state.fail_until {
        (state.fail_status, state.fail_body).into_response()
    } else {
        (StatusCode::OK, state.success_body).into_response()
    }
}

async fn handle_venue_error() -> Response {
    (
        StatusCode::BAD_REQUEST,
        r#"{"code":1001,"message":"invalid market"}"#,
    )
        .into_response()
}

async fn handle_method_not_allowed() -> Response {
    (StatusCode::METHOD_NOT_ALLOWED, "method not allowed").into_response()
}

async fn handle_body_rate_limit() -> Response {
    (
        StatusCode::BAD_REQUEST,
        r#"{"code":429,"message":"slow down"}"#,
    )
        .into_response()
}

async fn handle_success_body_venue_error() -> Response {
    (
        StatusCode::OK,
        r#"{"code":1001,"message":"invalid market","order_books":[]}"#,
    )
        .into_response()
}

async fn handle_success_body_rate_limit() -> Response {
    (
        StatusCode::OK,
        r#"{"code":429,"message":"slow down","next_cursor":null,"trades":[]}"#,
    )
        .into_response()
}

fn raw_client(base_url: String) -> LighterRawHttpClient {
    let mut client =
        LighterRawHttpClient::new(LighterEnvironment::Mainnet, Some(base_url), 10, None).unwrap();
    client.set_retry_manager(fast_retry_manager(3));
    client
}

// Compressed retry timings keep retry-path assertions in the millisecond range.
fn fast_retry_manager(max_retries: u32) -> RetryManager<LighterHttpError> {
    RetryManager::new(RetryConfig {
        max_retries,
        initial_delay_ms: 1,
        max_delay_ms: 1,
        backoff_factor: 1.0,
        jitter_ms: 0,
        operation_timeout_ms: Some(60_000),
        immediate_first: true,
        max_elapsed_ms: Some(60_000),
    })
}

fn assert_lighter_multipart_body<'a>(headers: &HeaderMap, body: &'a Bytes) -> &'a str {
    let content_type = headers
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    assert!(content_type.starts_with("multipart/form-data; boundary="));

    let body = std::str::from_utf8(body).unwrap();
    assert!(body.contains("--nautilus-lighter-form-boundary\r\n"));
    assert!(body.ends_with("--nautilus-lighter-form-boundary--\r\n"));
    body
}

fn assert_candles_query_common(query: &LighterCandlesQuery) {
    assert_eq!(query.market_id, 0);
    assert_eq!(query.resolution, LighterCandleResolution::OneMinute);
    assert_eq!(query.count_back, i64::from(LIGHTER_CANDLES_MAX_LIMIT));
    assert_eq!(query.set_timestamp_to_end, Some(false));
}

fn candles_response(candles: &[(i64, &str)]) -> String {
    let entries = candles
        .iter()
        .map(|(timestamp, close)| {
            format!(
                r#"{{"t":{timestamp},"o":{close},"h":{close},"l":{close},"c":{close},"v":1.0000,"V":100.0,"i":1}}"#
            )
        })
        .collect::<Vec<_>>()
        .join(",");

    format!(r#"{{"code":200,"r":"1m","c":[{entries}]}}"#)
}

fn fundings_response(rows: &[(i64, &str, &str)]) -> String {
    let entries = rows
        .iter()
        .map(|(timestamp, rate, direction)| {
            format!(
                r#"{{"timestamp":{timestamp},"value":"0.0","rate":"{rate}","direction":"{direction}"}}"#
            )
        })
        .collect::<Vec<_>>()
        .join(",");

    format!(r#"{{"code":200,"resolution":"1h","fundings":[{entries}]}}"#)
}

fn one_minute_bar_type(instrument_id: InstrumentId) -> BarType {
    BarType::new(
        instrument_id,
        BarSpecification::new(1, BarAggregation::Minute, PriceType::Last),
        AggregationSource::External,
    )
}

fn unsupported_three_minute_bar_type(instrument_id: InstrumentId) -> BarType {
    BarType::new(
        instrument_id,
        BarSpecification::new(3, BarAggregation::Minute, PriceType::Last),
        AggregationSource::External,
    )
}

fn millis_to_unix_nanos(timestamp_ms: i64) -> UnixNanos {
    let timestamp_ms = u64::try_from(timestamp_ms).unwrap();
    UnixNanos::from(timestamp_ms * 1_000_000)
}

async fn spawn_server(router: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    format!("http://{addr}")
}

fn create_test_instrument() -> InstrumentAny {
    let instrument_id = InstrumentId::new(Symbol::new("ETH-PERP"), Venue::new("LIGHTER"));

    InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
        instrument_id,
        Symbol::new("ETH-PERP"),
        Currency::from("ETH"),
        Currency::from("USDC"),
        Currency::from("USDC"),
        false,
        2,
        4,
        Price::from("0.01"),
        Quantity::from("0.0001"),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        UnixNanos::default(),
        UnixNanos::default(),
    ))
}

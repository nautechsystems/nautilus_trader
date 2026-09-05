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

//! Integration tests for the Ax HTTP client using a mock Axum server.

use std::{net::SocketAddr, path::PathBuf, time::Duration};

use axum::{Router, extract::Query, http::StatusCode, response::Json, routing::get};
use jiff::civil::Date;
use nautilus_architect_ax::{
    common::enums::AxCandleWidth,
    http::{
        client::{AxHttpClient, AxRawHttpClient},
        error::AxHttpError,
        query::{GetFundingRatesParams, GetFundingSlotsParams},
    },
};
use nautilus_common::testing::wait_until_async;
use nautilus_core::UnixNanos;
use nautilus_model::{
    identifiers::{AccountId, ClientOrderId, InstrumentId},
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::http::HttpClient;
use rstest::rstest;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde_json::{Value, json};
use ustr::Ustr;

use crate::common::server::start_test_server as start_common_test_server;

#[derive(Debug, Clone, Copy)]
enum RequiredInstrumentCachePath {
    BookSnapshot,
    Trades,
    Bars,
}

#[derive(Debug, Clone, Copy)]
enum ReportFamily {
    OpenOrders,
    HistoricalOrders,
    Fills,
    Positions,
}

async fn set_report_payload(
    state: &crate::common::server::TestServerState,
    family: ReportFamily,
    symbol: &str,
) {
    match family {
        ReportFamily::OpenOrders => {
            let first = json!({
                "tn": 500_000_000,
                "ts": 1_704_067_200,
                "d": "B",
                "o": "ACCEPTED",
                "oid": "OID-UNCACHED-1",
                "p": "1.08400",
                "q": 100,
                "rq": 100,
                "s": symbol,
                "tif": "GTC",
                "u": "u",
                "xq": 0,
                "cid": null,
                "tag": null,
                "po": true
            });
            let mut second = first.clone();
            second["oid"] = json!("OID-UNCACHED-2");
            second["tn"] = json!(600_000_000);
            *state.open_orders_payload.lock().await = Some(json!({
                "orders": [first, second]
            }));
        }
        ReportFamily::HistoricalOrders => {
            let first = json!({
                "ts": 1_704_067_200,
                "tn": 500_000_000,
                "oid": "OID-UNCACHED-1",
                "aid": "account-1",
                "u": "u",
                "s": symbol,
                "p": "1.08400",
                "q": 100,
                "xq": 100,
                "rq": 0,
                "o": "FILLED",
                "d": "B",
                "tif": "IOC",
                "cid": null,
                "r": null,
                "tag": null,
                "txt": null,
                "po": false
            });
            let mut second = first.clone();
            second["oid"] = json!("OID-UNCACHED-2");
            second["tn"] = json!(600_000_000);
            *state.orders_payload.lock().await = Some(json!({
                "orders": [first, second]
            }));
        }
        ReportFamily::Fills => {
            let first = json!({
                "trade_id": "T-UNCACHED-1",
                "order_id": "OID-UNCACHED-1",
                "fee": "0.10",
                "is_taker": true,
                "is_block_trade": false,
                "is_final_settlement": false,
                "price": "1.08410",
                "quantity": 100,
                "side": "B",
                "symbol": symbol,
                "timestamp": "2024-01-01T00:00:00Z",
                "account_id": "account-1"
            });
            let mut second = first.clone();
            second["trade_id"] = json!("T-UNCACHED-2");
            second["order_id"] = json!("OID-UNCACHED-2");
            *state.fills_payload.lock().await = Some(json!({
                "fills": [first, second]
            }));
        }
        ReportFamily::Positions => {
            let first = json!({
                "account_id": "account-1",
                "symbol": symbol,
                "signed_quantity": 100,
                "signed_notional": "108.40",
                "timestamp": "2024-01-01T00:00:00Z",
                "realized_pnl": "1.25"
            });
            let mut second = first.clone();
            second["signed_quantity"] = json!(-50);
            second["signed_notional"] = json!("-54.20");
            *state.positions_payload.lock().await = Some(json!({
                "positions": [first, second]
            }));
        }
    }
}

async fn request_report_instrument_ids(
    client: &AxHttpClient,
    family: ReportFamily,
) -> anyhow::Result<Vec<InstrumentId>> {
    let account_id = AccountId::from("AX-001");
    let instrument_ids = match family {
        ReportFamily::OpenOrders => client
            .request_order_status_reports(account_id, None::<fn(u64) -> Option<ClientOrderId>>)
            .await?
            .into_iter()
            .map(|report| report.instrument_id)
            .collect(),
        ReportFamily::HistoricalOrders => client
            .request_historical_order_status_reports(
                account_id,
                None,
                None,
                None::<fn(u64) -> Option<ClientOrderId>>,
            )
            .await?
            .into_iter()
            .map(|report| report.instrument_id)
            .collect(),
        ReportFamily::Fills => client
            .request_fill_reports(account_id, None, None)
            .await?
            .into_iter()
            .map(|report| report.instrument_id)
            .collect(),
        ReportFamily::Positions => client
            .request_position_reports(account_id)
            .await?
            .into_iter()
            .map(|report| report.instrument_id)
            .collect(),
    };
    Ok(instrument_ids)
}

/// Wait for the test server to be ready by polling a health endpoint.
async fn wait_for_server(addr: SocketAddr, path: &str) {
    let health_url = format!("http://{addr}{path}");
    let http_client = HttpClient::builder().build().unwrap();
    wait_until_async(
        || {
            let url = health_url.clone();
            let client = http_client.clone();
            async move { client.get(url, None, None, Some(1), None).await.is_ok() }
        },
        Duration::from_secs(5),
    )
    .await;
}

fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn load_test_data(filename: &str) -> Value {
    let path = manifest_path().join("test_data").join(filename);
    let content = std::fs::read_to_string(path).unwrap();
    serde_json::from_str(&content).unwrap()
}

async fn handle_funding_rates(
    Query(params): Query<GetFundingRatesParams>,
) -> Json<serde_json::Value> {
    let rates = (0..101)
        .map(|index| {
            json!({
                "symbol": params.symbol,
                "timestamp_ns": index,
                "funding_rate": "0.0001",
                "funding_amount": "0.10",
                "benchmark_price": "1.0845",
                "settlement_price": "1.0846"
            })
        })
        .collect::<Vec<_>>();
    let total_count = rates.len();
    let offset = params
        .cursor
        .as_deref()
        .and_then(|cursor| cursor.parse::<usize>().ok())
        .unwrap_or(0);
    let limit = params.limit.unwrap_or(100).max(0) as usize;
    let page = rates
        .into_iter()
        .skip(offset)
        .take(limit)
        .collect::<Vec<_>>();
    let next_offset = offset + page.len();
    let next_cursor = (next_offset < total_count).then(|| next_offset.to_string());

    Json(json!({
        "funding_rates": page,
        "total_count": total_count,
        "limit": limit,
        "next_cursor": next_cursor,
    }))
}

async fn handle_repeated_funding_cursor(
    Query(params): Query<GetFundingRatesParams>,
) -> Json<serde_json::Value> {
    let timestamp_ns = i64::from(params.cursor.is_some());

    Json(json!({
        "funding_rates": [{
            "symbol": params.symbol,
            "timestamp_ns": timestamp_ns,
            "funding_rate": "0.0001",
            "funding_amount": "0.10",
            "benchmark_price": "1.0845",
            "settlement_price": "1.0846"
        }],
        "limit": 1,
        "next_cursor": "same",
    }))
}

async fn handle_funding_slots(Query(params): Query<GetFundingSlotsParams>) -> Json<Value> {
    let mut payload = load_test_data("http_get_funding_slots.json");
    payload["symbol"] = json!(params.symbol.as_str());
    if let Some(date) = params.date {
        payload["date"] = json!(date);
    }
    Json(payload)
}

async fn handle_empty_open_orders_page() -> Json<serde_json::Value> {
    Json(json!({
        "orders": [],
        "total_count": 1,
        "limit": 100,
        "offset": 0,
    }))
}

fn create_router() -> Router {
    Router::new()
        .route(
            "/instruments",
            get(|| async { Json(load_test_data("http_get_instruments.json")) }),
        )
        .route(
            "/instrument",
            get(|| async {
                let data = load_test_data("http_get_instruments.json");
                let instruments = data["instruments"].as_array().unwrap();
                Json(instruments[0].clone())
            }),
        )
        .route(
            "/balances",
            get(|| async { Json(load_test_data("http_get_balances.json")) }),
        )
        .route(
            "/positions",
            get(|| async { Json(load_test_data("http_get_positions.json")) }),
        )
        .route(
            "/whoami",
            get(|| async { Json(load_test_data("http_get_whoami.json")) }),
        )
        .route(
            "/book",
            get(|| async { Json(load_test_data("http_get_book.json")) }),
        )
        .route(
            "/tickers",
            get(|| async {
                Json(json!({
                    "limit": 100,
                    "offset": 0,
                    "total_count": 1,
                    "tickers": [
                        {
                            "s": "BTC-PERP",
                            "bp": "45000.00",
                            "ap": "45001.00",
                            "p": "45000.50",
                            "m": "45000.25",
                            "q": 1,
                            "v": 1000000,
                            "oi": 10,
                            "ts": 1705314600,
                            "tn": 0
                        }
                    ]
                }))
            }),
        )
        .route(
            "/ticker",
            get(|| async {
                Json(json!({
                    "ticker": {
                        "s": "BTC-PERP",
                        "bp": "45000.00",
                        "ap": "45001.00",
                        "p": "45000.50",
                        "m": "45000.25",
                        "q": 1,
                        "v": 1000000,
                        "oi": 10,
                        "ts": 1705314600,
                        "tn": 0
                    }
                }))
            }),
        )
        .route("/funding-rates", get(handle_funding_rates))
        .route("/funding-slots", get(handle_funding_slots))
}

async fn start_test_server() -> SocketAddr {
    let addr = start_server(create_router()).await;
    wait_for_server(addr, "/instruments").await;
    addr
}

async fn start_server(router: Router) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });

    addr
}

#[rstest]
#[tokio::test]
async fn test_raw_http_get_instruments_returns_data() {
    let addr = start_test_server().await;
    let base_url = format!("http://{addr}");

    let client = AxRawHttpClient::new(Some(base_url), None, 60, 3, 1000, 10_000, None).unwrap();

    let response = client.get_instruments().await.unwrap();

    assert_eq!(response.instruments.len(), 3);
    assert_eq!(response.instruments[0].symbol.as_str(), "EURUSD-PERP");
    assert_eq!(response.instruments[1].symbol.as_str(), "XAU-PERP");
    assert_eq!(response.instruments[2].symbol.as_str(), "NVDA-PERP");
}

#[rstest]
#[tokio::test]
async fn test_raw_http_get_instrument_returns_data() {
    let addr = start_test_server().await;
    let base_url = format!("http://{addr}");

    let client = AxRawHttpClient::new(Some(base_url), None, 60, 3, 1000, 10_000, None).unwrap();

    // Mock server returns first instrument from list (EURUSD-PERP)
    let instrument = client
        .get_instrument(Ustr::from("EURUSD-PERP"))
        .await
        .unwrap();

    assert_eq!(instrument.symbol.as_str(), "EURUSD-PERP");
    assert_eq!(instrument.tick_size, dec!(0.0001));
}

#[rstest]
#[tokio::test]
async fn test_raw_http_get_balances_returns_data() {
    let addr = start_test_server().await;
    let base_url = format!("http://{addr}");

    let client = AxRawHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        Some(base_url),
        None,
        60,
        3,
        1000,
        10_000,
        None,
    )
    .unwrap();
    client.set_session_token("test_session_token".into());

    let response = client.get_balances().await.unwrap();

    assert_eq!(response.balances.len(), 3);
    assert_eq!(response.balances[0].symbol.as_str(), "USD");
    assert_eq!(response.balances[0].amount, dec!(100000.50));
    assert_eq!(response.balances[1].symbol.as_str(), "BTC");
    assert_eq!(response.balances[1].amount, dec!(1.25));
    assert_eq!(response.balances[2].symbol.as_str(), "ETH");
    assert_eq!(response.balances[2].amount, dec!(15.5));
}

#[rstest]
#[tokio::test]
async fn test_raw_http_get_positions_returns_data() {
    let addr = start_test_server().await;
    let base_url = format!("http://{addr}");

    let client = AxRawHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        Some(base_url),
        None,
        60,
        3,
        1000,
        10_000,
        None,
    )
    .unwrap();
    client.set_session_token("test_session_token".into());

    let response = client.get_positions().await.unwrap();

    assert_eq!(response.positions.len(), 2);
    assert_eq!(response.positions[0].symbol.as_str(), "BTC-PERP");
    assert_eq!(response.positions[0].signed_quantity, 2);
    assert_eq!(response.positions[0].signed_notional, dec!(90000.00));
    assert_eq!(response.positions[1].symbol.as_str(), "ETH-PERP");
    assert_eq!(response.positions[1].signed_quantity, -5);
}

#[rstest]
#[tokio::test]
async fn test_raw_http_get_tickers_returns_data() {
    let addr = start_test_server().await;
    let base_url = format!("http://{addr}");

    let client = AxRawHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        Some(base_url),
        None,
        60,
        3,
        1000,
        10_000,
        None,
    )
    .unwrap();
    client.set_session_token("test_session_token".into());

    let response = client.get_tickers().await.unwrap();

    assert_eq!(response.tickers.len(), 1);
    assert_eq!(response.total_count, 1);
    let ticker = &response.tickers[0];
    assert_eq!(ticker.symbol.as_str(), "BTC-PERP");
    assert_eq!(ticker.bid, Some(dec!(45000.00)));
    assert_eq!(ticker.ask, Some(dec!(45001.00)));
    assert_eq!(ticker.last, Some(dec!(45000.50)));
    assert_eq!(ticker.mark, Some(dec!(45000.25)));
}

#[rstest]
#[tokio::test]
async fn test_raw_http_get_ticker_returns_data() {
    let addr = start_test_server().await;
    let base_url = format!("http://{addr}");

    let client = AxRawHttpClient::with_credentials(
        "test_api_key".to_string(),
        "test_api_secret".to_string(),
        Some(base_url),
        None,
        60,
        3,
        1000,
        10_000,
        None,
    )
    .unwrap();
    client.set_session_token("test_session_token".into());

    let ticker = client.get_ticker(Ustr::from("BTC-PERP")).await.unwrap();

    assert_eq!(ticker.symbol.as_str(), "BTC-PERP");
    assert_eq!(ticker.bid, Some(dec!(45000.00)));
    assert_eq!(ticker.ask, Some(dec!(45001.00)));
}

#[rstest]
#[tokio::test]
async fn test_domain_http_request_instruments_returns_nautilus_types() {
    let addr = start_test_server().await;
    let base_url = format!("http://{addr}");

    let client = AxHttpClient::new(Some(base_url), None, 60, 3, 1000, 10_000, None).unwrap();

    let instruments = client
        .request_instruments(Some(Decimal::new(2, 4)), Some(Decimal::new(5, 4)))
        .await
        .unwrap();

    assert_eq!(instruments.len(), 3);
}

#[rstest]
#[tokio::test]
async fn test_domain_http_request_account_fees_reaches_instruments() {
    let addr = start_test_server().await;
    let base_url = format!("http://{addr}");

    let client = AxHttpClient::new(Some(base_url), None, 60, 3, 1000, 10_000, None).unwrap();
    client.set_session_token("test_session_token".into());

    let (maker_fee, taker_fee) = client.request_account_fees().await.unwrap();
    let instruments = client.request_instruments(None, None).await.unwrap();

    assert_eq!(maker_fee, dec!(0.0002));
    assert_eq!(taker_fee, dec!(0.0025));
    assert!(!instruments.is_empty());
    for instrument in &instruments {
        assert_eq!(instrument.maker_fee(), dec!(0.0002));
        assert_eq!(instrument.taker_fee(), dec!(0.0025));
    }
}

#[rstest]
#[case(Some(dec!(0.0001)), None, dec!(0.0001), dec!(0.0025))]
#[case(None, Some(dec!(0.0009)), dec!(0.0002), dec!(0.0009))]
#[tokio::test]
async fn test_domain_http_partial_fee_arguments_keep_resolved_rate_for_the_other_side(
    #[case] maker_arg: Option<Decimal>,
    #[case] taker_arg: Option<Decimal>,
    #[case] expected_maker: Decimal,
    #[case] expected_taker: Decimal,
) {
    let addr = start_test_server().await;
    let base_url = format!("http://{addr}");

    let client = AxHttpClient::new(Some(base_url), None, 60, 3, 1000, 10_000, None).unwrap();
    client.set_session_token("test_session_token".into());
    client.request_account_fees().await.unwrap();

    let instruments = client
        .request_instruments(maker_arg, taker_arg)
        .await
        .unwrap();

    assert!(!instruments.is_empty());
    for instrument in &instruments {
        assert_eq!(instrument.maker_fee(), expected_maker);
        assert_eq!(instrument.taker_fee(), expected_taker);
    }
}

#[rstest]
#[case(Some(dec!(0.0001)), None, dec!(0.0001), Decimal::ZERO)]
#[case(None, Some(dec!(0.0009)), Decimal::ZERO, dec!(0.0009))]
#[tokio::test]
async fn test_domain_http_partial_fee_arguments_zero_the_other_side_when_unresolved(
    #[case] maker_arg: Option<Decimal>,
    #[case] taker_arg: Option<Decimal>,
    #[case] expected_maker: Decimal,
    #[case] expected_taker: Decimal,
) {
    // The session token routes this through the authenticated branch of `resolve_fees`, which
    // warns; the warning text is not asserted, since the crate has no log-capture harness.
    let addr = start_test_server().await;
    let base_url = format!("http://{addr}");

    let client = AxHttpClient::new(Some(base_url), None, 60, 3, 1000, 10_000, None).unwrap();
    client.set_session_token("test_session_token".into());

    let instruments = client
        .request_instruments(maker_arg, taker_arg)
        .await
        .unwrap();

    assert!(!instruments.is_empty());
    for instrument in &instruments {
        assert_eq!(instrument.maker_fee(), expected_maker);
        assert_eq!(instrument.taker_fee(), expected_taker);
    }
}

#[rstest]
#[tokio::test]
async fn test_domain_http_resolved_fees_are_shared_across_clones() {
    // The instrument refresh task and the request handlers all run on clones of the client, so
    // resolved rates have to be shared state rather than per-clone state.
    let addr = start_test_server().await;
    let base_url = format!("http://{addr}");

    let client = AxHttpClient::new(Some(base_url), None, 60, 3, 1000, 10_000, None).unwrap();
    client.set_session_token("test_session_token".into());
    let cloned = client.clone();

    client.request_account_fees().await.unwrap();
    let instruments = cloned.request_instruments(None, None).await.unwrap();

    assert!(!instruments.is_empty());
    for instrument in &instruments {
        assert_eq!(instrument.maker_fee(), dec!(0.0002));
        assert_eq!(instrument.taker_fee(), dec!(0.0025));
    }
}

#[rstest]
#[tokio::test]
async fn test_domain_http_request_account_fees_uses_first_of_several_accounts() {
    // AX resolves the first account when a request carries no explicit selector
    let client = start_whoami_server(json!([
        whoami_account("01JBXR-7QK2-0000", "0.0002", "0.0025"),
        whoami_account("01JBXR-7QK2-0001", "0.0009", "0.0009"),
    ]))
    .await;

    let (maker_fee, taker_fee) = client.request_account_fees().await.unwrap();

    assert_eq!(maker_fee, dec!(0.0002));
    assert_eq!(taker_fee, dec!(0.0025));
}

#[rstest]
#[tokio::test]
async fn test_domain_http_request_account_fees_rejects_empty_accounts() {
    let client = start_whoami_server(json!([])).await;

    let error = client.request_account_fees().await.unwrap_err();

    assert_eq!(
        error.to_string(),
        "AX whoami returned no accounts to resolve fees from"
    );
}

#[rstest]
#[tokio::test]
async fn test_domain_http_request_account_fees_rejects_absent_rates() {
    // An absent rate must abort rather than resolve to zero, which is a valid rate
    let mut account = whoami_account("01JBXR-7QK2-0000", "0.0002", "0.0025");
    account["taker_fee"] = json!("");
    let client = start_whoami_server(json!([account])).await;

    let error = client.request_account_fees().await.unwrap_err();

    assert_eq!(
        error.to_string(),
        "AX whoami account 01JBXR-7QK2-0000 supplied no fee rates"
    );
}

#[rstest]
#[tokio::test]
async fn test_domain_http_request_account_fees_accepts_zero_rates() {
    // A promotional account can legitimately carry a zero maker rate
    let client =
        start_whoami_server(json!([whoami_account("01JBXR-7QK2-0000", "0", "0.0025")])).await;

    let (maker_fee, taker_fee) = client.request_account_fees().await.unwrap();

    assert_eq!(maker_fee, Decimal::ZERO);
    assert_eq!(taker_fee, dec!(0.0025));
}

async fn start_whoami_server(accounts: Value) -> AxHttpClient {
    let response = json!({
        "id": "01JBXR-7QK2-0000",
        "username": "trader@example.com",
        "created_at": "2025-12-18T02:20:42.675817Z",
        "is_onboarded": true,
        "is_frozen": false,
        "is_admin": false,
        "require_2fa": false,
        "accounts": accounts,
    });
    let router = Router::new().route("/whoami", get(|| async move { Json(response) }));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });

    wait_for_server(addr, "/whoami").await;

    let base_url = format!("http://{addr}");
    let client = AxHttpClient::new(Some(base_url), None, 60, 0, 1000, 10_000, None).unwrap();
    client.set_session_token("test_session_token".into());
    client
}

fn whoami_account(id: &str, maker_fee: &str, taker_fee: &str) -> Value {
    json!({
        "id": id,
        "name": "trader@example.com",
        "is_close_only": false,
        "maker_fee": maker_fee,
        "taker_fee": taker_fee,
        "can_list": true,
        "can_read": true,
        "can_set_limits": true,
        "can_reduce_or_close": true,
        "can_trade": true,
    })
}

#[rstest]
#[tokio::test]
async fn test_domain_http_request_instruments_reports_zero_fees_until_resolved() {
    let addr = start_test_server().await;
    let base_url = format!("http://{addr}");

    let client = AxHttpClient::new(Some(base_url), None, 60, 3, 1000, 10_000, None).unwrap();

    let instruments = client.request_instruments(None, None).await.unwrap();

    assert!(!instruments.is_empty());
    for instrument in &instruments {
        assert_eq!(instrument.maker_fee(), Decimal::ZERO);
        assert_eq!(instrument.taker_fee(), Decimal::ZERO);
    }
}

#[rstest]
#[tokio::test]
async fn test_domain_http_request_book_snapshot_composes_event_timestamp() {
    let addr = start_test_server().await;
    let base_url = format!("http://{addr}");

    let client = AxHttpClient::new(Some(base_url), None, 60, 3, 1000, 10_000, None).unwrap();
    client.set_session_token("test_session_token".into());

    let symbol = Ustr::from("EURUSD-PERP");
    let instrument = client.request_instrument(symbol, None, None).await.unwrap();
    client.cache_instrument(instrument);

    let book = client.request_book_snapshot(symbol, None).await.unwrap();

    // Fixture carries ts=1704067200 and tn=500000000
    assert_eq!(book.ts_last, UnixNanos::from(1_704_067_200_500_000_000u64));
    assert_eq!(book.instrument_id, InstrumentId::from("EURUSD-PERP.AX"));
}

#[rstest]
#[tokio::test]
async fn test_domain_http_request_book_snapshot_rejects_unrepresentable_price() {
    // A price beyond the fixed-point range must surface an error rather than silently
    // falling back to a wire-precision price
    let router = Router::new()
        .route(
            "/instrument",
            get(|| async {
                let data = load_test_data("http_get_instruments.json");
                Json(data["instruments"][0].clone())
            }),
        )
        .route(
            "/book",
            get(|| async {
                let mut data = load_test_data("http_get_book.json");
                data["book"]["b"][0]["p"] = json!("999999999999999999999999999");
                Json(data)
            }),
        );
    let addr = start_server(router).await;
    let base_url = format!("http://{addr}");

    let client = AxHttpClient::new(Some(base_url), None, 60, 3, 1000, 10_000, None).unwrap();
    client.set_session_token("test_session_token".into());
    let symbol = Ustr::from("EURUSD-PERP");
    let instrument = client
        .request_instrument(symbol, Some(Decimal::ZERO), Some(Decimal::ZERO))
        .await
        .unwrap();
    client.cache_instrument(instrument);

    let error = client
        .request_book_snapshot(symbol, None)
        .await
        .expect_err("unrepresentable price should error");

    assert!(
        error
            .to_string()
            .contains("Failed to convert AX book bid price"),
        "unexpected error: {error}"
    );
}

#[rstest]
#[tokio::test]
async fn test_domain_http_request_instrument_returns_nautilus_type() {
    let addr = start_test_server().await;
    let base_url = format!("http://{addr}");

    let client = AxHttpClient::new(Some(base_url), None, 60, 3, 1000, 10_000, None).unwrap();

    // Mock server returns first instrument (EURUSD-PERP) regardless of request
    let instrument = client
        .request_instrument(Ustr::from("EURUSD-PERP"), None, None)
        .await
        .unwrap();

    match instrument {
        InstrumentAny::PerpetualContract(perp) => {
            assert_eq!(perp.id.symbol.as_str(), "EURUSD-PERP");
            assert_eq!(perp.id.venue.as_str(), "AX");
            assert_eq!(perp.price_precision, 4);
            assert_eq!(perp.price_increment.as_decimal(), dec!(0.0001));
            assert_eq!(perp.quote_currency.code.as_str(), "USD");
            assert_eq!(perp.margin_init, dec!(0.08));
            assert_eq!(perp.margin_maint, dec!(0.04));
        }
        _ => panic!("Expected PerpetualContract instrument"),
    }
}

#[rstest]
#[tokio::test]
async fn test_domain_http_cache_instruments() {
    let addr = start_test_server().await;
    let base_url = format!("http://{addr}");

    let client = AxHttpClient::new(Some(base_url), None, 60, 3, 1000, 10_000, None).unwrap();

    assert!(!client.is_initialized());

    let instruments = client.request_instruments(None, None).await.unwrap();
    client.cache_instruments(&instruments);

    assert!(client.is_initialized());

    let cached_symbols = client.get_cached_symbols();
    assert_eq!(cached_symbols.len(), 3);
    assert!(cached_symbols.contains(&"EURUSD-PERP".to_string()));
    assert!(cached_symbols.contains(&"XAU-PERP".to_string()));
    assert!(cached_symbols.contains(&"NVDA-PERP".to_string()));
}

#[rstest]
#[tokio::test]
async fn test_domain_http_get_cached_instrument() {
    let addr = start_test_server().await;
    let base_url = format!("http://{addr}");

    let client = AxHttpClient::new(Some(base_url), None, 60, 3, 1000, 10_000, None).unwrap();

    let instruments = client.request_instruments(None, None).await.unwrap();
    client.cache_instruments(&instruments);

    let eurusd_symbol = Ustr::from("EURUSD-PERP");
    let cached = client.get_instrument(&eurusd_symbol);
    assert!(cached.is_some());

    let xau_symbol = Ustr::from("XAU-PERP");
    let cached = client.get_instrument(&xau_symbol);
    assert!(cached.is_some());

    let unknown_symbol = Ustr::from("UNKNOWN-PERP");
    let cached = client.get_instrument(&unknown_symbol);
    assert!(cached.is_none());
}

#[rstest]
#[tokio::test]
async fn test_domain_http_request_funding_rates_reads_all_cursor_pages() {
    let addr = start_test_server().await;
    let base_url = format!("http://{addr}");
    let client = AxHttpClient::new(Some(base_url), None, 60, 3, 1000, 10_000, None).unwrap();
    client.set_session_token("test_session_token".into());

    let updates = client
        .request_funding_rates(InstrumentId::from("EURUSD-PERP.AX"), None, None)
        .await
        .unwrap();

    assert_eq!(updates.len(), 101);
}

#[rstest]
#[tokio::test]
async fn test_domain_http_request_funding_rates_rejects_repeated_cursor() {
    let router = Router::new().route("/funding-rates", get(handle_repeated_funding_cursor));
    let addr = start_server(router).await;
    let base_url = format!("http://{addr}");
    let client = AxHttpClient::new(Some(base_url), None, 60, 3, 1000, 10_000, None).unwrap();
    client.set_session_token("test_session_token".into());

    let error = client
        .request_funding_rates(InstrumentId::from("EURUSD-PERP.AX"), None, None)
        .await
        .unwrap_err();

    assert_eq!(
        error.to_string(),
        "Parameter validation error: AX funding-rates pagination repeated cursor \"same\""
    );
}

#[rstest]
#[tokio::test]
async fn test_domain_http_request_funding_slots_returns_schedule() {
    let addr = start_test_server().await;
    let base_url = format!("http://{addr}");
    let client = AxHttpClient::new(Some(base_url), None, 60, 3, 1000, 10_000, None).unwrap();
    client.set_session_token("test_session_token".into());

    let date = Date::new(2026, 7, 6).unwrap();
    let response = client
        .request_funding_slots(InstrumentId::from("EURUSD-PERP.AX"), Some(date))
        .await
        .unwrap();

    assert_eq!(response.symbol, "EURUSD-PERP");
    assert_eq!(response.date, date);
    assert_eq!(response.slots.len(), 4);
    assert_eq!(response.realized_sum_bps, dec!(5.0921));
}

#[rstest]
#[tokio::test]
async fn test_domain_http_request_open_orders_rejects_empty_page_before_total() {
    let router = Router::new().route("/open-orders", get(handle_empty_open_orders_page));
    let addr = start_server(router).await;
    let base_url = format!("http://{addr}");
    let client = AxHttpClient::new(
        Some(base_url.clone()),
        Some(base_url),
        60,
        3,
        1000,
        10_000,
        None,
    )
    .unwrap();
    client.set_session_token("test_session_token".into());

    let error = client
        .request_order_status_reports(
            AccountId::from("AX-001"),
            None::<fn(u64) -> Option<ClientOrderId>>,
        )
        .await
        .unwrap_err();

    assert_eq!(
        error.to_string(),
        "AX open-orders returned an empty page before offset 0 reached total 1"
    );
}

#[rstest]
#[case::open_orders(ReportFamily::OpenOrders)]
#[case::historical_orders(ReportFamily::HistoricalOrders)]
#[case::fills(ReportFamily::Fills)]
#[case::positions(ReportFamily::Positions)]
#[tokio::test]
async fn test_domain_http_report_fetches_and_caches_uncached_instrument(
    #[case] family: ReportFamily,
) {
    let (addr, state) = start_common_test_server().await.unwrap();
    set_report_payload(&state, family, "GBPUSD-PERP").await;
    let base_url = format!("http://{addr}");
    let client =
        AxHttpClient::new(Some(base_url.clone()), Some(base_url), 60, 0, 1, 1, None).unwrap();
    client.set_session_token("test_session_token".into());
    client.request_account_fees().await.unwrap();

    let instrument_ids = request_report_instrument_ids(&client, family)
        .await
        .unwrap();
    let symbol = Ustr::from("GBPUSD-PERP");
    let instrument = client.get_instrument(&symbol).expect("cached instrument");

    assert_eq!(
        instrument_ids,
        vec![
            InstrumentId::from("GBPUSD-PERP.AX"),
            InstrumentId::from("GBPUSD-PERP.AX")
        ]
    );
    assert_eq!(instrument.id(), InstrumentId::from("GBPUSD-PERP.AX"));
    assert_eq!(instrument.maker_fee(), dec!(0.0002));
    assert_eq!(instrument.taker_fee(), dec!(0.0025));
    assert_eq!(
        request_report_instrument_ids(&client, family)
            .await
            .unwrap(),
        instrument_ids
    );
    let queries = state.instrument_queries.lock().await;
    assert_eq!(queries.len(), 1);
    assert_eq!(queries[0].symbol, symbol);
}

#[rstest]
#[tokio::test]
async fn test_domain_http_report_reuses_pre_cached_instrument() {
    let (addr, state) = start_common_test_server().await.unwrap();
    set_report_payload(&state, ReportFamily::OpenOrders, "EURUSD-PERP").await;
    let base_url = format!("http://{addr}");
    let client =
        AxHttpClient::new(Some(base_url.clone()), Some(base_url), 60, 0, 1, 1, None).unwrap();
    client.set_session_token("test_session_token".into());
    client.request_account_fees().await.unwrap();
    let instruments = client.request_instruments(None, None).await.unwrap();
    client.cache_instruments(&instruments);

    let instrument_ids = request_report_instrument_ids(&client, ReportFamily::OpenOrders)
        .await
        .unwrap();

    assert_eq!(
        instrument_ids,
        vec![
            InstrumentId::from("EURUSD-PERP.AX"),
            InstrumentId::from("EURUSD-PERP.AX")
        ]
    );
    assert!(state.instrument_queries.lock().await.is_empty());
}

// Error handling tests

#[rstest]
#[case::book_snapshot(RequiredInstrumentCachePath::BookSnapshot)]
#[case::trades(RequiredInstrumentCachePath::Trades)]
#[case::bars(RequiredInstrumentCachePath::Bars)]
#[tokio::test]
async fn test_public_market_data_request_missing_cached_instrument_returns_error(
    #[case] path: RequiredInstrumentCachePath,
) {
    let client = AxHttpClient::new(
        Some("http://127.0.0.1:9".to_string()),
        None,
        1,
        0,
        1,
        1,
        None,
    )
    .unwrap();
    let symbol = Ustr::from("BTC-PERP");

    let result = match path {
        RequiredInstrumentCachePath::BookSnapshot => {
            client.request_book_snapshot(symbol, None).await.map(|_| ())
        }
        RequiredInstrumentCachePath::Trades => client
            .request_trade_ticks(symbol, None, None, None)
            .await
            .map(|_| ()),
        RequiredInstrumentCachePath::Bars => client
            .request_bars(symbol, None, None, AxCandleWidth::Minutes1)
            .await
            .map(|_| ()),
    };

    assert_eq!(
        result.unwrap_err().to_string(),
        format!("Instrument {symbol} not found in cache")
    );
}

#[rstest]
#[tokio::test]
async fn test_http_network_error_invalid_port() {
    let base_url = "http://127.0.0.1:1".to_string();

    let client = AxRawHttpClient::new(Some(base_url), None, 1, 0, 1000, 10_000, None).unwrap();

    let result = client.get_instruments().await;

    assert!(result.is_err());
    match result {
        Err(AxHttpError::NetworkError(_)) => {}
        other => panic!("expected NetworkError: {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_http_500_internal_server_error() {
    let router = Router::new().route(
        "/instruments",
        get(|| async {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "Internal server error"
                })),
            )
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });

    wait_for_server(addr, "/instruments").await;

    let base_url = format!("http://{addr}");
    let client = AxRawHttpClient::new(Some(base_url), None, 60, 0, 1000, 10_000, None).unwrap();

    let result = client.get_instruments().await;

    assert!(result.is_err());
    match result {
        Err(AxHttpError::UnexpectedStatus { status, .. }) => {
            assert_eq!(status, 500);
        }
        other => panic!("expected UnexpectedStatus: {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_http_malformed_json_response() {
    let router = Router::new().route("/instruments", get(|| async { "not valid json" }));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });

    wait_for_server(addr, "/instruments").await;

    let base_url = format!("http://{addr}");
    let client = AxRawHttpClient::new(Some(base_url), None, 60, 3, 1000, 10_000, None).unwrap();

    let result = client.get_instruments().await;

    assert!(result.is_err());
    match result {
        Err(AxHttpError::JsonError(_)) => {}
        other => panic!("expected JsonError: {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_http_empty_instruments_response() {
    let router = Router::new().route(
        "/instruments",
        get(|| async {
            Json(json!({
                "instruments": []
            }))
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });

    wait_for_server(addr, "/instruments").await;

    let base_url = format!("http://{addr}");
    let client = AxRawHttpClient::new(Some(base_url), None, 60, 3, 1000, 10_000, None).unwrap();

    let result = client.get_instruments().await.unwrap();

    assert!(result.instruments.is_empty());
}

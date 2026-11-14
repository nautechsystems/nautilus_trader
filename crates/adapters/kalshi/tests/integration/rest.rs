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

use std::{process::Command, sync::Arc, time::Duration};

use aws_lc_rs::signature::{ED25519, UnparsedPublicKey};
use base64::prelude::*;
use nautilus_kalshi::{
    common::enums::{
        CandlestickInterval, MarketStatusFilter, MveHistoricalFilter, OrderStatusFilter,
        PositionSettlementStatus,
    },
    config::KalshiDataClientConfig,
    http::{
        client::KalshiHttpClient, error::KalshiHttpError, models::*, query::*, quota::KalshiQuota,
    },
};
use nautilus_network::{
    dst::{net::TcpListener, task, time},
    retry::RetryError,
};
use rstest::rstest;
use rust_decimal::Decimal;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[cfg(all(feature = "simulation", madsim))]
use super::dst as peer;
#[cfg(not(all(feature = "simulation", madsim)))]
use super::http as peer;
use super::keys;

#[rstest]
fn exact_response_fixtures_roundtrip_every_mapped_field() {
    fn roundtrip<T: Serialize + DeserializeOwned>(name: &str) {
        let value = fixture(name);
        let parsed: T = serde_json::from_slice(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value(parsed).unwrap(),
            value,
            "fixture {name}"
        );
    }
    roundtrip::<ExchangeStatus>("status");
    roundtrip::<MarketResponse>("market");
    roundtrip::<MarketsResponse>("markets");
    roundtrip::<EventResponse>("event");
    roundtrip::<EventsResponse>("events");
    roundtrip::<SeriesResponse>("series");
    roundtrip::<SeriesListResponse>("series_list");
    roundtrip::<OrderbookResponse>("orderbook");
    roundtrip::<TradesResponse>("trades");
    roundtrip::<CandlesticksResponse>("candlesticks");
    roundtrip::<HistoricalCandlesticksResponse>("historical_candlesticks");
    roundtrip::<HistoricalCutoff>("cutoff");
    roundtrip::<Balance>("balance");
    roundtrip::<PositionsResponse>("positions");
    roundtrip::<SettlementsResponse>("settlements");
    roundtrip::<FillsResponse>("fills");
    roundtrip::<OrdersResponse>("orders");
    roundtrip::<OrderResponse>("order");
    roundtrip::<AccountLimits>("limits");
    roundtrip::<EndpointCosts>("costs");
    roundtrip::<CreateOrderResponse>("create");
    roundtrip::<AmendOrderResponse>("amend");
    roundtrip::<CancelOrderResponse>("cancel");
    roundtrip::<DecreaseOrderResponse>("decrease");
    roundtrip::<BatchCreateResponse>("batch_create");
    roundtrip::<BatchCancelResponse>("batch_cancel");
    roundtrip::<MarketsResponse>("empty_markets");
    roundtrip::<TradesResponse>("empty_trades");
    roundtrip::<FillsResponse>("empty_fills");
    roundtrip::<OrdersResponse>("empty_orders");
    roundtrip::<PositionsResponse>("empty_positions");
    roundtrip::<SettlementsResponse>("empty_settlements");
}

#[rstest]
#[case("0.000000")]
#[case("-1234567890123456789.123456789")]
#[case("2.75")]
fn fixed_point_retains_exact_value_and_scale(#[case] text: &str) {
    let encoded = serde_json::to_vec(text).unwrap();
    let value: FixedPoint = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(value.0, Decimal::from_str_exact(text).unwrap());
    assert_eq!(
        serde_json::to_string(&value).unwrap(),
        serde_json::to_string(text).unwrap()
    );
}

#[rstest]
#[case("1e-2")]
#[case("NaN")]
#[case("+1")]
#[case("1.2.3")]
#[case(".1")]
#[case("1.")]
#[case("0.12345678901234567890123456789")]
fn invalid_fixed_point_is_rejected(#[case] text: &str) {
    assert!(serde_json::from_slice::<FixedPoint>(&serde_json::to_vec(text).unwrap()).is_err());
}

#[rstest]
fn field_presence_and_unknown_order_status_are_preserved() {
    let mut value = fixture("amend");
    value.as_object_mut().unwrap().remove("remaining_count");
    let parsed: AmendOrderResponse =
        serde_json::from_slice(&serde_json::to_vec(&value).unwrap()).unwrap();
    assert_eq!(parsed.remaining_count, Field::Missing);
    assert_eq!(parsed.average_fill_price, Field::Null);
    assert_eq!(parsed.average_fee_paid, Field::Value(fp("0.000002")));
    assert_eq!(serde_json::to_value(parsed).unwrap(), value);
    let mut value = fixture("order");
    value["order"]["status"] = Value::String("future_state".into());
    let parsed: OrderResponse =
        serde_json::from_slice(&serde_json::to_vec(&value).unwrap()).unwrap();
    assert_eq!(parsed.order.status, OrderStatus::Unknown);
}

#[rstest]
#[case("status", "GET", "/exchange/status", false, "status")]
#[case(
    "markets",
    "GET",
    "/markets?limit=2&event_ticker=E%26X&min_close_ts=0&status=open",
    false,
    "markets"
)]
#[case("market", "GET", "/markets/A%2FB%3F%23", false, "market")]
#[case(
    "historical_markets",
    "GET",
    "/historical/markets?limit=2&tickers=A%2CB&mve_filter=exclude",
    false,
    "markets"
)]
#[case(
    "historical_market",
    "GET",
    "/historical/markets/TEST-MARKET",
    false,
    "market"
)]
#[case(
    "events",
    "GET",
    "/events?limit=2&with_nested_markets=false&series_ticker=S%2BX",
    false,
    "events"
)]
#[case(
    "event",
    "GET",
    "/events/TEST-EVENT?exclude_markets=false&with_nested_markets=true",
    false,
    "event"
)]
#[case(
    "series_list",
    "GET",
    "/series?category=Economics&tags=&include_volume=false&min_updated_ts=0",
    false,
    "series_list"
)]
#[case("series", "GET", "/series/TEST-SERIES", false, "series")]
#[case(
    "orderbook",
    "GET",
    "/markets/TEST-MARKET/orderbook?depth=0",
    false,
    "orderbook"
)]
#[case(
    "trades",
    "GET",
    "/markets/trades?limit=2&ticker=T%26X&min_ts=0&max_ts=123&is_block_trade=false",
    false,
    "trades"
)]
#[case(
    "historical_trades",
    "GET",
    "/historical/trades?limit=2&ticker=T%26X&min_ts=0&max_ts=123&is_block_trade=false",
    false,
    "trades"
)]
#[case(
    "candlesticks",
    "GET",
    "/series/TEST-SERIES/markets/TEST-MARKET/candlesticks?start_ts=0&end_ts=123&period_interval=60",
    false,
    "candlesticks"
)]
#[case(
    "historical_candlesticks",
    "GET",
    "/historical/markets/TEST-MARKET/candlesticks?start_ts=0&end_ts=123&period_interval=60",
    false,
    "historical_candlesticks"
)]
#[case("cutoff", "GET", "/historical/cutoff", false, "cutoff")]
#[case(
    "balance",
    "GET",
    "/portfolio/balance?subaccount=0&exchange_index=3",
    true,
    "balance"
)]
#[case(
    "positions",
    "GET",
    "/portfolio/positions?limit=2&settlement_status=all&subaccount=0&exchange_index=3",
    true,
    "positions"
)]
#[case(
    "historical_positions",
    "GET",
    "/historical/positions?event_ticker=E%26X&limit=2&subaccount=0",
    true,
    "positions"
)]
#[case(
    "fills",
    "GET",
    "/portfolio/fills?order_id=O%2BX&min_ts=0&limit=2&subaccount=0&exchange_index=3",
    true,
    "fills"
)]
#[case(
    "historical_fills",
    "GET",
    "/historical/fills?ticker=T%26X&min_ts=0&max_ts=123&limit=2&subaccount=0",
    true,
    "fills"
)]
#[case(
    "orders",
    "GET",
    "/portfolio/orders?event_ticker=E%26X&status=resting&limit=2&subaccount=0&exchange_index=3",
    true,
    "orders"
)]
#[case("order", "GET", "/portfolio/orders/order-a", true, "order")]
#[case(
    "historical_orders",
    "GET",
    "/historical/orders?ticker=T%26X&min_ts=0&max_ts=123&limit=2&subaccount=0",
    true,
    "orders"
)]
#[case(
    "settlements",
    "GET",
    "/portfolio/settlements?event_ticker=E%26X&max_ts=123&limit=2&subaccount=0",
    true,
    "settlements"
)]
#[case("limits", "GET", "/account/limits", true, "limits")]
#[case("costs", "GET", "/account/endpoint_costs", false, "costs")]
#[case("create", "POST", "/portfolio/events/orders", true, "create")]
#[case(
    "amend",
    "POST",
    "/portfolio/events/orders/order-a/amend?subaccount=0",
    true,
    "amend"
)]
#[case(
    "decrease",
    "POST",
    "/portfolio/events/orders/order-a/decrease?subaccount=0",
    true,
    "decrease"
)]
#[case(
    "cancel",
    "DELETE",
    "/portfolio/events/orders/order-a?subaccount=0&exchange_index=3",
    true,
    "cancel"
)]
#[case(
    "batch_create",
    "POST",
    "/portfolio/events/orders/batched",
    true,
    "batch_create"
)]
#[case(
    "batch_cancel",
    "DELETE",
    "/portfolio/events/orders/batched",
    true,
    "batch_cancel"
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn typed_endpoints_match_exact_wire_and_outputs(
    #[case] operation: &str,
    #[case] method: &str,
    #[case] path: &str,
    #[case] authenticated: bool,
    #[case] response_name: &str,
) {
    Box::pin(time::timeout(Duration::from_secs(5), async {
        let (listener, address) = peer::bind_peer(18200).await;
        let body = serde_json::to_vec(&fixture(response_name)).unwrap();

        let discover = !matches!(operation, "limits" | "costs");
        let server = task::spawn(async move {
            if discover {
                quota_discovery(&listener).await;
            }

            let (mut stream, _) = listener.accept().await.unwrap();
            let headers = peer::read_headers(&mut stream).await;
            let length = header(&headers, "content-length")
                .unwrap_or("0")
                .parse::<usize>()
                .unwrap();
            let mut request_body = vec![0; length];
            stream.read_exact(&mut request_body).await.unwrap();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
            stream.write_all(&body).await.unwrap();
            stream.flush().await.unwrap();
            (headers, request_body)
        });

        let client = test_client(address);
        let response = call(&client, operation, 0).await.unwrap();
        let (headers, body) = server.await.unwrap();
        assert_eq!(
            headers.lines().next().unwrap(),
            format!("{method} /custom/v3{path} HTTP/1.1")
        );
        assert_eq!(response, fixture(response_name));

        if authenticated {
            let path = format!("/custom/v3{}", path.split('?').next().unwrap());
            assert_signature(&headers, method, &path);
        } else {
            assert_eq!(header(&headers, "kalshi-access-key"), None);
        }

        if matches!(method, "POST") || operation == "batch_cancel" {
            let expected: Value =
                serde_json::from_str(include_str!("../../test_data/rest_requests.json")).unwrap();
            assert_eq!(
                serde_json::from_slice::<Value>(&body).unwrap(),
                expected[operation]
            );
            assert_eq!(header(&headers, "content-type"), Some("application/json"));
        } else {
            assert_eq!(body, Vec::<u8>::new());
        }
    }))
    .await
    .unwrap();
}

#[rstest]
#[case("balance", "balance")]
#[case("positions", "positions")]
#[case("historical_positions", "positions")]
#[case("fills", "fills")]
#[case("historical_fills", "fills")]
#[case("orders", "orders")]
#[case("historical_orders", "orders")]
#[case("settlements", "settlements")]
#[case("amend", "amend")]
#[case("decrease", "decrease")]
#[case("cancel", "cancel")]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn numbered_account_endpoints_preserve_scope(
    #[case] operation: &'static str,
    #[case] response: &'static str,
) {
    Box::pin(time::timeout(Duration::from_secs(5), async {
        let (listener, address) = peer::bind_peer(0).await;
        let server = task::spawn(async move {
            quota_discovery(&listener).await;
            reply(&listener, 200, fixture(response)).await
        });

        let result = call(&test_client(address), operation, 7).await.unwrap();
        let request = server.await.unwrap();
        let target = request
            .lines()
            .next()
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap();
        let url = url::Url::parse(&format!("http://localhost{target}")).unwrap();
        let scope = url
            .query_pairs()
            .find(|(name, _)| name == "subaccount")
            .unwrap()
            .1;
        assert_eq!(scope, "7");
        assert_eq!(result, fixture(response));
    }))
    .await
    .unwrap();
}

async fn call(
    client: &KalshiHttpClient,
    operation: &str,
    subaccount: u32,
) -> Result<Value, KalshiHttpError> {
    macro_rules! value {
        ($call:expr) => {
            serde_json::to_value($call.await?).map_err(|_| KalshiHttpError::Encoding)
        };
    }

    let trades = TradesQuery {
        limit: Some(2),
        ticker: Some("T&X".into()),
        min_ts: Some(0),
        max_ts: Some(123),
        is_block_trade: Some(false),
        ..Default::default()
    };

    let candles = CandlesticksQuery {
        start_ts: 0,
        end_ts: 123,
        period_interval: CandlestickInterval::Hour,
    };

    let account = AccountQuery {
        subaccount,
        exchange_index: 3,
    };

    let history = HistoryQuery {
        subaccount,
        ticker: Some("T&X".into()),
        min_ts: Some(0),
        max_ts: Some(123),
        limit: Some(2),
        ..Default::default()
    };

    let mut create = create_request();
    create.subaccount = subaccount;
    let mut batch_create = batch_create_request();
    for order in &mut batch_create.orders {
        order.subaccount = subaccount;
    }

    let mut batch_cancel = batch_cancel_request();
    for order in &mut batch_cancel.orders {
        order.subaccount = subaccount;
    }

    match operation {
        "status" => value!(client.get_status()),
        "markets" => value!(client.get_markets(&MarketsQuery {
            limit: Some(2),
            event_ticker: Some("E&X".into()),
            min_close_ts: Some(0),
            status: Some(MarketStatusFilter::Open),
            ..Default::default()
        })),
        "market" => value!(client.get_market("A/B?#")),
        "historical_markets" => value!(client.get_historical_markets(&HistoricalMarketsQuery {
            limit: Some(2),
            tickers: Some("A,B".into()),
            mve_filter: Some(MveHistoricalFilter::Exclude),
            ..Default::default()
        })),
        "historical_market" => value!(client.get_historical_market("TEST-MARKET")),
        "events" => value!(client.get_events(&EventsQuery {
            limit: Some(2),
            with_nested_markets: Some(false),
            series_ticker: Some("S+X".into()),
            ..Default::default()
        })),
        "event" => value!(client.get_event(
            "TEST-EVENT",
            &EventQuery {
                exclude_markets: Some(false),
                with_nested_markets: Some(true)
            }
        )),
        "series_list" => value!(client.get_series_list(&SeriesQuery {
            category: Some("Economics".into()),
            tags: Some(String::new()),
            include_volume: Some(false),
            min_updated_ts: Some(0)
        })),
        "series" => value!(client.get_series("TEST-SERIES")),
        "orderbook" => value!(client.get_orderbook("TEST-MARKET", 0)),
        "trades" => value!(client.get_trades(&trades)),
        "historical_trades" => value!(client.get_historical_trades(&trades)),
        "candlesticks" => value!(client.get_candlesticks("TEST-SERIES", "TEST-MARKET", &candles)),
        "historical_candlesticks" => {
            value!(client.get_historical_candlesticks("TEST-MARKET", &candles))
        }
        "cutoff" => value!(client.get_historical_cutoff()),
        "balance" => value!(client.get_account_balance(&account)),
        "positions" => value!(client.get_positions(&PositionsQuery {
            subaccount,
            limit: Some(2),
            settlement_status: Some(PositionSettlementStatus::All),
            exchange_index: 3,
            ..Default::default()
        })),
        "historical_positions" => {
            value!(client.get_historical_positions(&HistoricalPositionsQuery {
                subaccount,
                event_ticker: Some("E&X".into()),
                limit: Some(2),
                ..Default::default()
            }))
        }
        "fills" => value!(client.get_fills(&FillsQuery {
            subaccount,
            order_id: Some("O+X".into()),
            min_ts: Some(0),
            limit: Some(2),
            exchange_index: 3,
            ..Default::default()
        })),
        "historical_fills" => value!(client.get_historical_fills(&history)),
        "orders" => value!(client.get_orders(&OrdersQuery {
            subaccount,
            event_ticker: Some("E&X".into()),
            status: Some(OrderStatusFilter::Resting),
            limit: Some(2),
            exchange_index: 3,
            ..Default::default()
        })),
        "order" => value!(client.get_order("order-a")),
        "historical_orders" => value!(client.get_historical_orders(&history)),
        "settlements" => value!(client.get_settlements(&SettlementsQuery {
            subaccount,
            event_ticker: Some("E&X".into()),
            max_ts: Some(123),
            limit: Some(2),
            ..Default::default()
        })),
        "limits" => value!(client.get_account_limits()),
        "costs" => value!(client.get_endpoint_costs()),
        "create" => value!(client.create_order(&create)),
        "amend" => value!(client.amend_order(
            "order-a",
            subaccount,
            &AmendOrderRequest {
                ticker: "TEST-MARKET".into(),
                side: BookSide::Ask,
                price: fp("0.4321"),
                count: fp("3.25"),
                exchange_index: 3,
                client_order_id: Some("client-a".into()),
                updated_client_order_id: Some("client-b".into()),
                expiration_time: Some(1792112600)
            }
        )),
        "decrease" => value!(client.decrease_order(
            "order-a",
            subaccount,
            &DecreaseOrderRequest {
                exchange_index: 3,
                reduce_by: None,
                reduce_to: Some(fp("0.00")),
                market_ticker: Some("TEST-MARKET".into())
            }
        )),
        "cancel" => value!(client.cancel_order("order-a", &account)),
        "batch_create" => value!(client.batch_create_orders(&batch_create)),
        "batch_cancel" => value!(client.batch_cancel_orders(&batch_cancel)),
        _ => panic!("unknown test operation {operation}"),
    }
}

fn create_request() -> CreateOrderRequest {
    CreateOrderRequest {
        ticker: "TEST-MARKET".into(),
        side: BookSide::Bid,
        count: fp("2.75"),
        price: fp("0.1234"),
        time_in_force: TimeInForce::GoodTillCanceled,
        self_trade_prevention_type: SelfTradePrevention::Maker,
        subaccount: 0,
        exchange_index: 3,
        client_order_id: Some("client-a".into()),
        expiration_time: None,
        post_only: Some(false),
        cancel_order_on_pause: Some(true),
        reduce_only: None,
    }
}

fn batch_create_request() -> BatchCreateRequest {
    let first = create_request();
    let mut second = create_request();
    second.client_order_id = Some("client-c".into());
    second.side = BookSide::Ask;
    second.price = fp("0.4321");
    second.count = fp("3.25");

    BatchCreateRequest {
        orders: vec![first, second],
    }
}

fn batch_cancel_request() -> BatchCancelRequest {
    BatchCancelRequest {
        orders: ["order-a", "order-b"]
            .into_iter()
            .map(|id| CancelOrderRequest {
                order_id: id.into(),
                subaccount: 0,
                exchange_index: 3,
                market_ticker: None,
            })
            .collect(),
    }
}

pub(super) fn test_client(address: std::net::SocketAddr) -> KalshiHttpClient {
    KalshiHttpClient::new_authenticated(&KalshiDataClientConfig {
        api_key_id: Some("auth-test-id".into()),
        private_key: Some(keys::ed25519_pkcs8().into()),
        base_url_http: Some(format!("http://{address}/custom/prefix/../v3").into()),
        http_timeout_secs: 1,
        ..Default::default()
    })
    .unwrap()
}

fn fixture(name: &str) -> Value {
    let fixtures: Value =
        serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
    fixtures.get(name).unwrap().clone()
}

fn fp(text: &str) -> FixedPoint {
    FixedPoint(Decimal::from_str_exact(text).unwrap())
}

fn header<'a>(headers: &'a str, name: &str) -> Option<&'a str> {
    headers.lines().find_map(|line| {
        let (key, value) = line.split_once(": ")?;
        key.eq_ignore_ascii_case(name).then_some(value)
    })
}

fn assert_signature(headers: &str, method: &str, path: &str) {
    assert_eq!(header(headers, "kalshi-access-key"), Some("auth-test-id"));
    let timestamp = header(headers, "kalshi-access-timestamp")
        .unwrap()
        .parse::<u64>()
        .unwrap();
    let signature = BASE64_STANDARD
        .decode(header(headers, "kalshi-access-signature").unwrap())
        .unwrap();
    let public = include_bytes!("../../test_data/auth_ed25519_public.der");
    let payload = format!("{timestamp}{method}{path}");
    UnparsedPublicKey::new(&ED25519, &public[public.len() - 32..])
        .verify(payload.as_bytes(), &signature)
        .unwrap();
}

#[rstest]
#[case("create")]
#[case("amend")]
#[case("decrease")]
#[case("cancel")]
#[case("batch_create")]
#[case("batch_cancel")]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn mutations_are_single_send_on_transient_status_and_transport_failure(
    #[case] operation: &str,
) {
    Box::pin(time::timeout(Duration::from_secs(5), async {
        for disconnect in [false,true] {
            let (listener,address) = peer::bind_peer(18201).await;
            let (completed, completion) = tokio::sync::oneshot::channel();

            let server = task::spawn(async move {
            quota_discovery(&listener).await;
                let (mut stream,_) = listener.accept().await.unwrap();
                let headers = peer::read_headers(&mut stream).await;
                let length = header(&headers,"content-length").unwrap_or("0").parse::<usize>().unwrap();
                let mut body = vec![0;length];
                stream.read_exact(&mut body).await.unwrap();

                if !disconnect {
                    let body = serde_json::to_string(&fixture("error")).unwrap();
                    let response = format!("HTTP/1.1 503 Unavailable\r\nRetry-After: 4\r\nX-Request-Id: synthetic-id\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
                    stream.write_all(response.as_bytes()).await.unwrap();
                    stream.flush().await.unwrap();
                }

                drop(stream);
                peer::assert_no_connection(&listener, address, async { completion.await.unwrap(); }).await;
            });

            let error = call(&test_client(address),operation, 0).await.unwrap_err();
            completed.send(()).unwrap();
            server.await.unwrap();

            if disconnect { assert!(matches!(error,KalshiHttpError::Transport(_))); }
            else {
                assert!(!format!("{error:?}").contains("synthetic"));
                assert!(!error.to_string().contains("private"));

                match error {
                    KalshiHttpError::Venue { status,evidence } => {
                        assert_eq!(status,503);
                        assert_eq!(evidence.retry_after.as_deref(),Some("4"));
                        assert_eq!(evidence.request_id.as_deref(),Some("synthetic-id"));
                        assert_eq!(evidence.venue.unwrap(),serde_json::from_value::<VenueError>(fixture("error")["error"].clone()).unwrap());
                        assert_eq!(serde_json::from_str::<Value>(evidence.body.expose_secret()).unwrap(),fixture("error"));
                    }
                    _ => panic!("unexpected error: {error:?}"),
                }
            }
        }
    })).await.unwrap();
}

#[rstest]
#[case(200, "error")]
#[case(204, "empty_trades")]
#[case(302, "error")]
#[case(401, "error")]
#[case(403, "error")]
#[case(429, "error")]
#[case(500, "error")]
#[case(503, "error")]
#[case(504, "error")]
#[case(503, "html")]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn typed_errors_preserve_status_body_and_redaction(#[case] status: u16, #[case] name: &str) {
    Box::pin(time::timeout(Duration::from_secs(5),async {
        let (listener,address)=peer::bind_peer(18202).await;

        let body = match (status, name) {
            (204, _) => String::new(),
            (_, "html") => "<html>synthetic private error</html>".into(),
            _ => serde_json::to_string(&fixture(name)).unwrap(),
        };

        let expected=body.clone();

        let server=task::spawn(async move {
            quota_discovery(&listener).await;
            for _ in 0..if matches!(status,500 | 502..=504) { 3 } else { 1 } {
            let (mut stream,_)=listener.accept().await.unwrap();peer::read_headers(&mut stream).await;
            let response=format!("HTTP/1.1 {status} Response\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
            stream.write_all(response.as_bytes()).await.unwrap();stream.flush().await.unwrap();
            }
        });

        let error=test_client(address).get_status().await.unwrap_err();server.await.unwrap();
        assert!(!format!("{error:?}").contains("synthetic"));

        match error {
            KalshiHttpError::Decode {status:actual,evidence,..} if status<300 => {assert_eq!(actual,status);assert_eq!(evidence.body.expose_secret(),expected);}
            KalshiHttpError::Venue {status:actual,evidence} if status>=300 => {assert_eq!(actual,status);assert_eq!(evidence.body.expose_secret(),expected);}
            KalshiHttpError::Http {status:actual,evidence} if status>=300 && name=="html" => {assert_eq!(actual,status);assert_eq!(evidence.body.expose_secret(),expected);}
            _=>panic!("unexpected error: {error:?}"),
        }
    })).await.unwrap();
}

#[rstest]
#[case(false, "exhausted")]
#[case(true, "exhausted")]
#[case(false, "null")]
#[case(true, "empty")]
#[case(false, "budget")]
#[case(true, "budget")]
#[case(false, "repeat")]
#[case(true, "cycle")]
#[case(false, "empty_cursor")]
#[case(true, "duplicates")]
#[case(false, "source_failure")]
#[case(true, "source_failure")]
#[case(false, "malformed")]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn bounded_pagination_proves_progress_filters_dedup_and_completeness(
    #[case] archive: bool,
    #[case] mode: &str,
) {
    Box::pin(time::timeout(Duration::from_secs(5),async {
        let (listener,address)=peer::bind_peer(18203).await;

        let count=if mode=="budget" {1} else if mode=="cycle" {3} else {2};
        let mode_owned=mode.to_owned();

        let server=task::spawn(async move {
            quota_discovery(&listener).await;
            let mut requests=Vec::new();

            for page in 0..count {
                let (mut stream,_)=listener.accept().await.unwrap();
                let headers=peer::read_headers(&mut stream).await;
                requests.push(headers.lines().next().unwrap().to_owned());
                let mut body=fixture("trades");
                let original=body["trades"][0].clone();
                if page==0 {
                    let mut second=original;second["trade_id"]=Value::String("trade-b".into());body["trades"].as_array_mut().unwrap().push(second);
                    body["cursor"]=Value::String("A+&".into());
                } else {
                    body["trades"][0]["trade_id"]=Value::String("trade-b".into());
                    if mode_owned!="duplicates" {
                        let mut next=original;next["trade_id"]=Value::String(format!("trade-{page}"));body["trades"].as_array_mut().unwrap().push(next);
                    }

                    match mode_owned.as_str() {
                        "null"=>body["cursor"]=Value::Null,
                        "empty"=>{body["trades"]=Value::Array(Vec::new());body["cursor"]=Value::String(String::new());}
                        "repeat"=>body["cursor"]=Value::String("A+&".into()),
                        "cycle"=>body["cursor"]=Value::String(if page==1 {"B"} else {"A+&"}.into()),
                        "empty_cursor"=>{body["trades"]=Value::Array(Vec::new());body["cursor"]=Value::String("B".into());}
                        "duplicates"=>body["cursor"]=Value::String("B".into()),
                        "source_failure"=>body=fixture("error"),
                        "malformed"=>{body.as_object_mut().unwrap().remove("trades");}
                        _=>(),
                    }
                }

                let status=if mode_owned=="source_failure" && page==1 {403} else {200};
                let body=serde_json::to_string(&body).unwrap();
                let response=format!("HTTP/1.1 {status} Response\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
                stream.write_all(response.as_bytes()).await.unwrap();stream.flush().await.unwrap();
            }

            requests
        });

        let client=test_client(address);
        let query=TradesQuery {limit:Some(2),ticker:Some("T&X".into()),min_ts:Some(0),max_ts:Some(123),is_block_trade:Some(false),..Default::default()};
        let result=if archive {client.get_all_historical_trades(query,count).await} else {client.get_all_trades(query,count).await};
        let requests=server.await.unwrap();

        let endpoint=if archive {"historical/trades"} else {"markets/trades"};

        for (i,request) in requests.iter().enumerate() {
            let cursor=if i==0 {""} else if i==1 {"cursor=A%2B%26&"} else {"cursor=B&"};
            assert_eq!(request,&format!("GET /custom/v3/{endpoint}?limit=2&{cursor}ticker=T%26X&min_ts=0&max_ts=123&is_block_trade=false HTTP/1.1"));
        }

        if matches!(mode,"repeat"|"cycle"|"empty_cursor"|"duplicates") { assert!(matches!(result,Err(KalshiHttpError::Pagination(_)))); }
        else if mode=="source_failure" {assert!(matches!(result,Err(KalshiHttpError::Venue{status:403,..})));}
        else if mode=="malformed" {assert!(matches!(result,Err(KalshiHttpError::Decode{status:200,..})));}
        else {
            let result=result.unwrap();
            let ids=result.records.iter().map(|t|t.trade_id.as_str()).collect::<Vec<_>>();
            assert_eq!(ids,if matches!(mode,"empty"|"budget") {vec!["trade-a","trade-b"]} else {vec!["trade-a","trade-b","trade-1"]});
            assert_eq!(result.complete,mode!="budget");
            assert_eq!(result.next_cursor.as_deref(),if mode=="budget" {Some("A+&")} else {None});
        }
    })).await.unwrap();
}

#[rstest]
#[case(false)]
#[case(true)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn cancellation_distinguishes_before_dispatch_from_pending_request(#[case] mutation: bool) {
    Box::pin(time::timeout(Duration::from_secs(5), async {
        let (listener, address) = peer::bind_peer(18204).await;
        let client = test_client(address);
        let clone = client.clone();
        clone.cancel();
        assert!(matches!(
            client.create_order(&create_request()).await,
            Err(KalshiHttpError::Canceled { dispatched: false })
        ));
        peer::assert_no_connection(&listener, address, std::future::ready(())).await;
        let client = test_client(address);
        let clone = client.clone();
        let (ready, received) = tokio::sync::oneshot::channel();

        let server = task::spawn(async move {
            quota_discovery(&listener).await;
            let (mut stream, _) = listener.accept().await.unwrap();
            peer::read_headers(&mut stream).await;
            ready.send(()).unwrap();
            let mut remaining = Vec::new();
            stream.read_to_end(&mut remaining).await.unwrap();
        });

        let operation = if mutation { "create" } else { "status" };
        let request = task::spawn(async move { call(&client, operation, 0).await });
        received.await.unwrap();
        clone.cancel();
        assert!(matches!(
            request.await.unwrap(),
            Err(KalshiHttpError::Canceled { dispatched: true })
        ));
        server.await.unwrap();
    }))
    .await
    .unwrap();
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn invalid_orders_and_scope_do_not_reach_transport() {
    let (listener, address) = peer::bind_peer(18205).await;
    let client = test_client(address);
    let mut invalid = create_request();
    invalid.subaccount = 64;
    assert!(matches!(
        client.create_order(&invalid).await,
        Err(KalshiHttpError::Validation(_))
    ));
    invalid = create_request();
    invalid.reduce_only = Some(true);
    assert!(matches!(
        client.create_order(&invalid).await,
        Err(KalshiHttpError::Validation(_))
    ));
    invalid = create_request();
    invalid.count = fp("0.001");
    assert!(matches!(
        client.create_order(&invalid).await,
        Err(KalshiHttpError::Validation(_))
    ));

    let invalid = DecreaseOrderRequest {
        exchange_index: 3,
        reduce_by: Some(fp("1.00")),
        reduce_to: Some(fp("0.00")),
        market_ticker: None,
    };

    assert!(matches!(
        client.decrease_order("order-a", 0, &invalid).await,
        Err(KalshiHttpError::Validation(_))
    ));
    assert!(matches!(
        client.get_all_trades(TradesQuery::default(), 0).await,
        Err(KalshiHttpError::Validation(_))
    ));
    assert!(matches!(
        client.get_market("..").await,
        Err(KalshiHttpError::Validation(_))
    ));
    peer::assert_no_connection(&listener, address, std::future::ready(())).await;
}

#[rstest]
#[case("0.5", "0.5000")]
#[case("0.56", "0.5600")]
#[case("0.1234", "0.1234")]
fn order_prices_use_four_decimal_places_without_rounding(
    #[case] input: &str,
    #[case] output: &str,
) {
    let mut request = create_request();
    request.price = fp(input);
    let value = serde_json::to_value(&request).unwrap();
    let expected: Value =
        serde_json::from_str(include_str!("../../test_data/rest_requests.json")).unwrap();
    let mut expected = expected["create"].clone();
    expected["price"] = Value::String(output.into());
    assert_eq!(value, expected);
    request.price = fp("0.12345");
    assert!(serde_json::to_value(request).is_err());
}

#[rstest]
fn exact_json_numbers_never_round_through_binary_floats() {
    let values: std::collections::BTreeMap<String, DecimalNumber> =
        serde_json::from_str(include_str!("../../test_data/rest_numbers.json")).unwrap();
    assert_eq!(values.len(), 3);
    assert_eq!(
        values["positive"].0,
        Decimal::from_str_exact("0.1234567890123456789012345678").unwrap()
    );
    assert_eq!(
        values["negative"].0,
        Decimal::from_str_exact("-9876543210987654321.123456789").unwrap()
    );
    assert_eq!(
        values["scientific"].0,
        Decimal::from_str_exact("0.1234567890123456789012345678").unwrap()
    );
    assert_eq!(
        serde_json::to_string(&values["positive"]).unwrap(),
        "0.1234567890123456789012345678"
    );
}

#[rstest]
#[case("0.12345678901234567890123456789e0")]
#[case("-0.12345678901234567890123456789E0")]
#[case("1e-29")]
#[case("\"1e0\"")]
fn exact_json_numbers_reject_rounding_and_non_numeric_tokens(#[case] text: &str) {
    assert!(serde_json::from_str::<DecimalNumber>(text).is_err());
}

#[rstest]
#[case("0.12345678901234567890123456789e1", "1.2345678901234567890123456789")]
#[case("123456789012345678901234567890e-2", "1234567890123456789012345678.9")]
fn exact_json_numbers_normalize_representable_scientific_values(
    #[case] text: &str,
    #[case] expected: &str,
) {
    let value: DecimalNumber = serde_json::from_str(text).unwrap();
    assert_eq!(value.0, Decimal::from_str_exact(expected).unwrap());
}

#[rstest]
#[case("tags")]
#[case("additional_prohibitions")]
fn required_nullable_series_fields_reject_absence(#[case] field: &str) {
    let mut value = fixture("series");
    value["series"][field] = Value::Null;
    let parsed: SeriesResponse =
        serde_json::from_slice(&serde_json::to_vec(&value).unwrap()).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), value);
    value["series"].as_object_mut().unwrap().remove(field);
    assert!(
        serde_json::from_slice::<SeriesResponse>(&serde_json::to_vec(&value).unwrap()).is_err()
    );
}

#[rstest]
fn batch_response_debug_redacts_venue_error_fields() {
    let create: BatchCreateResponse = serde_json::from_value(fixture("batch_create")).unwrap();
    let cancel: BatchCancelResponse = serde_json::from_value(fixture("batch_cancel")).unwrap();
    assert_eq!(
        create.orders[1].error,
        Field::Value(VenueError {
            code: Field::Value("permission_denied".into()),
            message: Field::Value("synthetic private text".into()),
            details: Field::Value("test details".into()),
        })
    );

    for formatted in [format!("{create:?}"), format!("{cancel:?}")] {
        for value in [
            "synthetic private text",
            "synthetic rejected cancel",
            "test detail",
            "permission_denied",
            "not_found",
        ] {
            assert!(!formatted.contains(value));
        }
    }
}

#[rstest]
#[case(500)]
#[case(503)]
#[case(504)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn maintenance_responses_keep_typed_exchange_status(#[case] status: u16) {
    Box::pin(time::timeout(Duration::from_secs(5), async {
        let (listener, address) = peer::bind_peer(18206).await;
        let expected = fixture("status");
        let body = expected.clone();

        let server = task::spawn(async move {
            quota_discovery(&listener).await;
            let mut headers = String::new();
            for _ in 0..3 {
                headers = reply(&listener, status, body.clone()).await;
            }

            headers
        });

        let response = test_client(address).get_status().await.unwrap();
        server.await.unwrap();
        assert_eq!(serde_json::to_value(response).unwrap(), expected);
    }))
    .await
    .unwrap();
}

#[rstest]
#[case(true, "reordered")]
#[case(true, "count")]
#[case(true, "duplicate")]
#[case(true, "foreign")]
#[case(true, "missing")]
#[case(false, "reordered")]
#[case(false, "count")]
#[case(false, "duplicate")]
#[case(false, "foreign")]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn batch_results_match_request_identities_or_preserve_ambiguous_evidence(
    #[case] create: bool,
    #[case] mode: &str,
) {
    Box::pin(time::timeout(Duration::from_secs(5), async {
        let (listener, address) = peer::bind_peer(18207).await;

        let operation = if create {
            "batch_create"
        } else {
            "batch_cancel"
        };

        let field = if create {
            "client_order_id"
        } else {
            "order_id"
        };

        let mut body = fixture(operation);
        match mode {
            "reordered" => body["orders"].as_array_mut().unwrap().reverse(),
            "count" => {
                body["orders"].as_array_mut().unwrap().pop();
            }
            "duplicate" => body["orders"][1][field] = body["orders"][0][field].clone(),
            "foreign" => body["orders"][1][field] = Value::String("not-requested".into()),
            "missing" => body["orders"][1][field] = Value::Null,
            _ => panic!("unknown test mode"),
        }

        let response_body = body.clone();

        let server = task::spawn(async move {
            quota_discovery(&listener).await;
            reply(&listener, 200, response_body).await
        });

        let result = call(&test_client(address), operation, 0).await;
        server.await.unwrap();

        if mode == "reordered" {
            assert_eq!(result.unwrap(), body);
        } else {
            match result.unwrap_err() {
                KalshiHttpError::Response {
                    status, evidence, ..
                } => {
                    assert_eq!(status, 200);
                    assert_eq!(
                        serde_json::from_str::<Value>(evidence.body.expose_secret()).unwrap(),
                        body
                    );
                }
                error => panic!("unexpected error: {error:?}"),
            }
        }
    }))
    .await
    .unwrap();
}

#[rstest]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn invalid_batch_identities_never_dispatch() {
    let (listener, address) = peer::bind_peer(18208).await;
    let client = test_client(address);
    let mut create = batch_create_request();
    create.orders[1].client_order_id = create.orders[0].client_order_id.clone();
    assert!(matches!(
        client.batch_create_orders(&create).await,
        Err(KalshiHttpError::Validation(_))
    ));
    create.orders[1].client_order_id = None;
    assert!(matches!(
        client.batch_create_orders(&create).await,
        Err(KalshiHttpError::Validation(_))
    ));
    create.orders[1].client_order_id = Some(String::new());
    assert!(matches!(
        client.batch_create_orders(&create).await,
        Err(KalshiHttpError::Validation(_))
    ));
    let mut cancel = batch_cancel_request();
    cancel.orders[1].order_id = cancel.orders[0].order_id.clone();
    assert!(matches!(
        client.batch_cancel_orders(&cancel).await,
        Err(KalshiHttpError::Validation(_))
    ));
    peer::assert_no_connection(&listener, address, std::future::ready(())).await;
}

#[rstest]
#[case(false, "both")]
#[case(true, "both")]
#[case(false, "market")]
#[case(true, "event")]
#[case(false, "empty")]
#[case(true, "empty")]
#[case(false, "budget")]
#[case(true, "budget")]
#[case(false, "repeat")]
#[case(true, "repeat")]
#[case(false, "duplicates")]
#[case(true, "duplicates")]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn position_pages_preserve_both_lists_and_bound_progress(
    #[case] archive: bool,
    #[case] mode: &str,
) {
    Box::pin(time::timeout(Duration::from_secs(5), async {
        let (listener, address) = peer::bind_peer(18209).await;
        let mut first = fixture("positions");
        if mode == "market" { first["event_positions"] = Value::Array(Vec::new()); }
        if mode == "event" { first["market_positions"] = Value::Array(Vec::new()); }
        let mut expected: PositionsResponse = serde_json::from_value(first.clone()).unwrap();
        first["cursor"] = Value::String("next+&".into());
        let mut second = first.clone();

        if !matches!(mode, "repeat" | "duplicates") {
            if mode != "event" {
                let mut market = second["market_positions"][0].clone();
                market["exchange_index"] = Value::Number(4.into());
                second["market_positions"].as_array_mut().unwrap().push(market.clone());
                expected.market_positions.push(serde_json::from_value(market).unwrap());
            }

            if mode != "market" {
                let mut event = second["event_positions"][0].clone();
                event["event_ticker"] = Value::String("EVENT-B".into());
                second["event_positions"].as_array_mut().unwrap().push(event.clone());
                expected.event_positions.push(serde_json::from_value(event).unwrap());
            }
        }

        second["cursor"] = Value::String(if mode == "repeat" { "next+&" } else if mode == "duplicates" { "again" } else { "" }.into());
        if mode == "empty" {
            second = fixture("empty_positions");
            expected = serde_json::from_value(fixture("positions")).unwrap();
        }

        let budget = if mode == "budget" { 1 } else { 2 };
        if mode == "budget" { expected = serde_json::from_value(fixture("positions")).unwrap(); }
        let mut bodies = vec![first];
        if budget == 2 { bodies.push(second); }

        let server = task::spawn(async move {
            quota_discovery(&listener).await;
            let mut requests = Vec::new();
            for body in bodies { requests.push(reply(&listener, 200, body).await); }
            requests
        });

        let client = test_client(address);
        let result = if archive {
            client.get_all_historical_positions(HistoricalPositionsQuery { event_ticker:Some("E&X".into()),limit:Some(2),..Default::default() },budget).await
        } else {
            client.get_all_positions(PositionsQuery { limit:Some(2),settlement_status:Some(PositionSettlementStatus::All),exchange_index:3,..Default::default() },budget).await
        };

        let requests = server.await.unwrap();
        for (index, request) in requests.iter().enumerate() {
            let path = if archive {
                let cursor = if index == 0 { "" } else { "cursor=next%2B%26&" };
                format!("/historical/positions?event_ticker=E%26X&limit=2&{cursor}subaccount=0")
            } else {
                let cursor = if index == 0 { "" } else { "cursor=next%2B%26&" };
                format!("/portfolio/positions?{cursor}limit=2&settlement_status=all&subaccount=0&exchange_index=3")
            };

            assert_eq!(request.lines().next().unwrap(),format!("GET /custom/v3{path} HTTP/1.1"));
        }

        if matches!(mode,"repeat" | "duplicates") {
            assert!(matches!(result,Err(KalshiHttpError::Pagination(_))));
        } else {
            let result = result.unwrap();
            assert_eq!(result.market_positions,expected.market_positions);
            assert_eq!(result.event_positions,expected.event_positions);
            assert_eq!(result.complete,mode != "budget");
            assert_eq!(result.next_cursor.as_deref(),if mode == "budget" {Some("next+&")} else {None});
        }
    })).await.unwrap();
}

pub(super) async fn reply(listener: &TcpListener, status: u16, body: Value) -> String {
    let (mut stream, _) = listener.accept().await.unwrap();
    let headers = peer::read_headers(&mut stream).await;
    let length = header(&headers, "content-length")
        .unwrap_or("0")
        .parse::<usize>()
        .unwrap();
    let mut bytes = vec![0; length];
    stream.read_exact(&mut bytes).await.unwrap();
    let body = serde_json::to_string(&body).unwrap();
    let response = format!(
        "HTTP/1.1 {status} Response\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).await.unwrap();
    stream.flush().await.unwrap();
    headers
}

pub(super) async fn quota_discovery(listener: &TcpListener) {
    for (path, fixture_name) in [
        ("/account/limits", "limits"),
        ("/account/endpoint_costs", "costs"),
    ] {
        let request = reply(listener, 200, fixture(fixture_name)).await;
        let target = request
            .lines()
            .next()
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap();
        assert!(
            target.ends_with(path),
            "unexpected bootstrap target: {target}"
        );
    }
}

#[rstest]
#[case(429, "rate_limit", 3)]
#[case(429, "rate_limit_code", 3)]
#[case(429, "rate_limit_other_code", 1)]
#[case(503, "malformed_success", 3)]
#[case(429, "malformed_success", 1)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn reads_count_retries_without_rate_limit_headers(
    #[case] status: u16,
    #[case] response_name: &str,
    #[case] attempts: usize,
) {
    Box::pin(time::timeout(Duration::from_secs(8), async {
        let (listener, address) = peer::bind_peer(18210).await;
        let (completed, completion) = tokio::sync::oneshot::channel();
        let body = quota_fixture(response_name);

        let server = task::spawn(async move {
            quota_discovery(&listener).await;
            for _ in 0..attempts {
                let request = reply(&listener, status, body.clone()).await;
                assert!(request.starts_with("GET /custom/v3/markets/trades"));
            }

            peer::assert_no_connection(&listener, address, async {
                completion.await.unwrap();
            })
            .await;

            attempts
        });

        let error = test_client(address)
            .get_trades(&TradesQuery::default())
            .await
            .unwrap_err();
        completed.send(()).unwrap();
        let count = server.await.unwrap();
        assert_eq!(count, attempts);
        assert_eq!(error.is_retryable(), attempts == 3);
        assert!(!error.is_definitely_not_sent());

        match error {
            KalshiHttpError::Http {
                status: actual,
                evidence,
            }
            | KalshiHttpError::Venue {
                status: actual,
                evidence,
            } => {
                assert_eq!(actual, status);
                assert_eq!(evidence.retry_after, None);
                assert_eq!(
                    serde_json::from_str::<Value>(evidence.body.expose_secret()).unwrap(),
                    quota_fixture(response_name)
                );
            }
            other => panic!("unexpected failure: {other:?}"),
        }
    }))
    .await
    .unwrap();
}

#[rstest]
#[case("create")]
#[case("amend")]
#[case("decrease")]
#[case("cancel")]
#[case("batch_create")]
#[case("batch_cancel")]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn unsafe_writes_send_once_and_preserve_uncertain_outcomes(#[case] operation: &str) {
    Box::pin(time::timeout(Duration::from_secs(8), async {
        for (status, name) in [
            (429, "rate_limit"),
            (429, "rate_limit_code"),
            (409, "duplicate"),
            (200, "malformed_success"),
        ] {
            let (listener, address) = peer::bind_peer(18211).await;
            let (completed, completion) = tokio::sync::oneshot::channel();

            let server = task::spawn(async move {
                quota_discovery(&listener).await;
                let headers = reply(&listener, status, quota_fixture(name)).await;
                assert!(headers.starts_with("POST ") || headers.starts_with("DELETE "));
                peer::assert_no_connection(&listener, address, async {
                    completion.await.unwrap();
                })
                .await;
            });

            let error = call(&test_client(address), operation, 0).await.unwrap_err();
            completed.send(()).unwrap();
            server.await.unwrap();
            assert!(
                !error.is_definitely_not_sent(),
                "lost write ambiguity for {operation}: {error:?}"
            );
            assert_eq!(error.is_retryable(), status == 429);
            assert!(!format!("{error:?}").contains("synthetic"));
        }
    }))
    .await
    .unwrap();
}

#[rstest]
#[case(false)]
#[case(true)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn failed_limit_discovery_keeps_reads_and_prevents_writes(#[case] failed_costs: bool) {
    Box::pin(time::timeout(Duration::from_secs(5), async {
        let (listener, address) = peer::bind_peer(18212).await;
        let (completed, completion) = tokio::sync::oneshot::channel();

        let server = task::spawn(async move {
            assert!(
                reply(
                    &listener,
                    if failed_costs { 200 } else { 403 },
                    if failed_costs {
                        fixture("limits")
                    } else {
                        quota_fixture("malformed_success")
                    }
                )
                .await
                .contains("/account/limits ")
            );
            assert!(
                reply(
                    &listener,
                    if failed_costs { 403 } else { 200 },
                    if failed_costs {
                        quota_fixture("malformed_success")
                    } else {
                        fixture("costs")
                    }
                )
                .await
                .contains("/account/endpoint_costs ")
            );
            assert!(
                reply(&listener, 200, fixture("status"))
                    .await
                    .contains("/exchange/status ")
            );
            peer::assert_no_connection(&listener, address, async {
                completion.await.unwrap();
            })
            .await;
        });

        let client = test_client(address);
        let discovery = client.get_status().await.unwrap_err();
        assert!(matches!(discovery, KalshiHttpError::Discovery(_)));
        assert!(discovery.is_definitely_not_sent());
        client.get_status().await.unwrap();
        let error = client.create_order(&create_request()).await.unwrap_err();
        completed.send(()).unwrap();
        server.await.unwrap();
        assert!(matches!(error, KalshiHttpError::Quota(_)));
        assert!(error.is_definitely_not_sent());
    }))
    .await
    .unwrap();
}

#[rstest]
#[case(false)]
#[case(true)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn restricted_key_discovers_costs_and_submits_with_basic_limits(#[case] invalid_costs: bool) {
    Box::pin(time::timeout(Duration::from_secs(5), async {
        let (listener, address) = peer::bind_peer(0).await;

        let mut costs = fixture("costs");
        if invalid_costs {
            costs["default_cost"] = 0.into();
        }

        let server = task::spawn(async move {
            assert!(
                reply(&listener, 403, quota_fixture("subaccount_restricted"))
                    .await
                    .contains("/account/limits ")
            );
            assert!(
                reply(&listener, 200, costs)
                    .await
                    .contains("/account/endpoint_costs ")
            );

            if invalid_costs {
                return;
            }

            assert!(
                reply(&listener, 200, fixture("create"))
                    .await
                    .contains("/portfolio/events/orders ")
            );
        });

        let response = test_client(address).create_order(&create_request()).await;
        if invalid_costs {
            let e = response.unwrap_err();
            assert!(matches!(e, KalshiHttpError::Discovery(_)));
            assert!(e.is_definitely_not_sent());
        } else {
            assert_eq!(
                response.unwrap(),
                serde_json::from_value(fixture("create")).unwrap()
            );
        }

        server.await.unwrap();
    }))
    .await
    .unwrap();
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn cancellation_during_backoff_retains_attempt_evidence() {
    Box::pin(time::timeout(Duration::from_secs(5), async {
        let (listener, address) = peer::bind_peer(18213).await;
        let (completed, completion) = tokio::sync::oneshot::channel();
        let client = test_client(address);
        let cancel = client.clone();

        let server = task::spawn(async move {
            quota_discovery(&listener).await;
            reply(&listener, 429, quota_fixture("rate_limit")).await;
            cancel.cancel();
            peer::assert_no_connection(&listener, address, async {
                completion.await.unwrap();
            })
            .await;
        });

        let error = client
            .get_trades(&TradesQuery::default())
            .await
            .unwrap_err();
        completed.send(()).unwrap();
        server.await.unwrap();
        assert!(matches!(
            error,
            KalshiHttpError::Canceled { dispatched: true }
        ));
        assert!(!error.is_definitely_not_sent());
    }))
    .await
    .unwrap();
}

fn quota_fixture(name: &str) -> Value {
    if matches!(name, "rate_limit_code" | "rate_limit_other_code") {
        let (code, message) = if name == "rate_limit_code" {
            ("too_many_requests", "synthetic quota diagnostic")
        } else {
            ("not_a_rate_limit", "too many requests")
        };

        return serde_json::json!({"error": {"code": code, "message": message}});
    }

    let fixtures: Value =
        serde_json::from_str(include_str!("../../test_data/quota_responses.json")).unwrap();
    fixtures[name].clone()
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn account_callers_share_quota_across_keys_and_transport_lifetimes() {
    Box::pin(time::timeout(Duration::from_secs(8), async {
        let (listener, address) = peer::bind_peer(18214).await;

        let server = task::spawn(async move {
            let mut limits = fixture("limits");
            limits["read"]["refill_rate"] = 100.into();
            limits["read"]["bucket_capacity"] = 100.into();
            assert!(
                reply(&listener, 200, limits)
                    .await
                    .contains("/account/limits ")
            );
            let mut costs = fixture("costs");
            costs["default_cost"] = 50.into();
            assert!(
                reply(&listener, 200, costs)
                    .await
                    .contains("/account/endpoint_costs ")
            );
            let start = time::Instant::now();
            let mut requests = Vec::new();
            for _ in 0..4 {
                requests.push(reply(&listener, 200, fixture("status")).await);
            }

            assert!(start.elapsed() >= Duration::from_millis(1_400));
            assert!(
                requests
                    .iter()
                    .all(|request| request.starts_with("GET /custom/v3/exchange/status "))
            );
            requests.len()
        });

        let quota = Arc::new(KalshiQuota::new());

        let config = KalshiDataClientConfig {
            api_key_id: Some("shared-key-a".into()),
            private_key: Some(keys::ed25519_pkcs8().into()),
            base_url_http: Some(format!("http://{address}/custom/v3").into()),
            ..Default::default()
        };

        let first = KalshiHttpClient::new_authenticated_with_quota(&config, quota.clone()).unwrap();
        first.get_status().await.unwrap();
        let public = KalshiHttpClient::new_with_quota(&config, quota.clone()).unwrap();
        public.get_status().await.unwrap();

        let config = KalshiDataClientConfig {
            api_key_id: Some("shared-key-b".into()),
            ..config
        };

        let second =
            KalshiHttpClient::new_authenticated_with_quota(&config, quota.clone()).unwrap();
        second.get_status().await.unwrap();
        drop(first);
        drop(public);
        drop(second);
        drop(quota);
        let recreated = KalshiHttpClient::new_authenticated(&config).unwrap();
        recreated.get_status().await.unwrap();
        assert_eq!(server.await.unwrap(), 4);
    }))
    .await
    .unwrap();
}

#[rstest]
#[case(false, 75)]
#[case(true, 50)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn elapsed_budget_preserves_before_and_after_send_evidence(
    #[case] sent: bool,
    #[case] cost: i64,
) {
    Box::pin(time::timeout(Duration::from_secs(8), async {
        let (listener, address) = peer::bind_peer(18215).await;
        let (finished, done) = tokio::sync::oneshot::channel();

        let server = task::spawn(async move {
            let mut limits = fixture("limits");
            limits["read"]["refill_rate"] = 1.into(); limits["read"]["bucket_capacity"] = 100.into();
            assert!(reply(&listener, 200, limits).await.contains("/account/limits "));
            let mut costs = fixture("costs"); costs["default_cost"] = cost.into();
            assert!(reply(&listener, 200, costs).await.contains("/account/endpoint_costs "));

            if sent { assert!(reply(&listener, 503, quota_fixture("malformed_success")).await.contains("/exchange/status ")); }
            tokio::select! {
                biased;
                attempt = listener.accept() => panic!("unexpected further transmission: {attempt:?}"),
                () = async { done.await.unwrap(); } => (),
            }
        });

        let error = test_client(address).get_status().await.unwrap_err();
        finished.send(()).unwrap(); server.await.unwrap();
        assert!(matches!(error, KalshiHttpError::Retry { reason: RetryError::ElapsedBudgetExceeded { .. }, dispatched } if dispatched == sent));
        assert_eq!(error.is_definitely_not_sent(), !sent);
    })).await.unwrap();
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn oversized_batch_never_dispatches_partial_budget() {
    Box::pin(time::timeout(Duration::from_secs(5), async {
        let (listener, address) = peer::bind_peer(18216).await;
        let (finished, done) = tokio::sync::oneshot::channel();

        let server = task::spawn(async move {
            let mut limits = fixture("limits");
            limits["write"]["bucket_capacity"] = 20.into();
            assert!(
                reply(&listener, 200, limits)
                    .await
                    .contains("/account/limits ")
            );
            assert!(
                reply(&listener, 200, fixture("costs"))
                    .await
                    .contains("/account/endpoint_costs ")
            );
            tokio::select! {
                biased;
                attempt = listener.accept() => panic!("oversized batch transmitted: {attempt:?}"),
                () = async { done.await.unwrap(); } => (),
            }
        });

        let error = test_client(address)
            .batch_create_orders(&batch_create_request())
            .await
            .unwrap_err();
        finished.send(()).unwrap();
        server.await.unwrap();
        assert!(matches!(
            error,
            KalshiHttpError::Quota("request token cost exceeds bucket capacity")
        ));
        assert!(error.is_definitely_not_sent());
    }))
    .await
    .unwrap();
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn later_local_admission_failure_preserves_prior_transmission() {
    Box::pin(time::timeout(Duration::from_secs(5), async {
        let (listener, address) = peer::bind_peer(18217).await;
        let (replied, first_reply) = tokio::sync::oneshot::channel();
        let (finished, done) = tokio::sync::oneshot::channel();

        let server = task::spawn(async move {
            quota_discovery(&listener).await;
            assert!(reply(&listener, 503, quota_fixture("malformed_success")).await.contains("/exchange/status "));
            replied.send(()).unwrap();
            let mut costs = fixture("costs");
            costs["default_cost"] = 1_000.into();
            assert!(reply(&listener, 200, costs).await.contains("/account/endpoint_costs "));
            tokio::select! {
                biased;
                attempt = listener.accept() => panic!("unfunded retry transmitted: {attempt:?}"),
                () = async { done.await.unwrap(); } => (),
            }
        });

        let client = test_client(address);
        let reader = client.clone();
        let read = task::spawn(async move { reader.get_status().await });
        first_reply.await.unwrap();
        client.get_endpoint_costs().await.unwrap();
        let error = read.await.unwrap().unwrap_err();
        finished.send(()).unwrap();
        server.await.unwrap();
        assert!(!error.is_definitely_not_sent(), "later local error erased transmission: {error:?}");
        assert!(matches!(&error, KalshiHttpError::AfterSend(source) if matches!(source.as_ref(), KalshiHttpError::Quota("request token cost exceeds bucket capacity"))));
    })).await.unwrap();
}

#[rstest]
#[case(false)]
#[case(true)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn invalid_discovery_values_preserve_received_response_evidence(#[case] costs: bool) {
    Box::pin(time::timeout(Duration::from_secs(5), async {
        let (listener, address) = peer::bind_peer(18218).await;
        let mut body = fixture(if costs { "costs" } else { "limits" });
        if costs { body["default_cost"] = 0.into(); } else { body["read"]["refill_rate"] = 0.into(); }
        let server = task::spawn(async move { reply(&listener, 200, body).await });
        let client = test_client(address);
        let error = if costs { client.get_endpoint_costs().await.unwrap_err() } else { client.get_account_limits().await.unwrap_err() };
        let request = server.await.unwrap();
        assert!(request.contains(if costs { "/account/endpoint_costs " } else { "/account/limits " }));
        assert!(!error.is_definitely_not_sent());
        assert!(matches!(&error, KalshiHttpError::AfterSend(source) if matches!(source.as_ref(), KalshiHttpError::Quota(reason) if *reason == if costs { "invalid venue endpoint costs" } else { "nonpositive venue token limit" })));
    })).await.unwrap();
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn canceled_discovery_does_not_poison_shared_owner() {
    Box::pin(time::timeout(Duration::from_secs(5), async {
        let (listener, address) = peer::bind_peer(18219).await;
        let (started, pending) = tokio::sync::oneshot::channel();
        let (resume, resumed) = tokio::sync::oneshot::channel();

        let server = task::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            assert!(
                peer::read_headers(&mut stream)
                    .await
                    .contains("/account/limits ")
            );
            started.send(()).unwrap();
            resumed.await.unwrap();
            drop(stream);
            quota_discovery(&listener).await;
            assert!(
                reply(&listener, 200, fixture("status"))
                    .await
                    .contains("/exchange/status ")
            );
        });

        let quota = Arc::new(KalshiQuota::new());

        let config = KalshiDataClientConfig {
            api_key_id: Some("canceled-discovery".into()),
            private_key: Some(keys::ed25519_pkcs8().into()),
            base_url_http: Some(format!("http://{address}/test").into()),
            ..Default::default()
        };

        let canceled =
            KalshiHttpClient::new_authenticated_with_quota(&config, quota.clone()).unwrap();
        let mut config_other = config.clone();
        config_other.api_key_id = Some("healthy-discovery".into());
        let healthy = KalshiHttpClient::new_authenticated_with_quota(&config_other, quota).unwrap();
        let refresh_client = canceled.clone();
        let refresh = task::spawn(async move { refresh_client.refresh_quota().await });
        pending.await.unwrap();
        canceled.cancel();
        let error = refresh.await.unwrap().unwrap_err();
        resume.send(()).unwrap();
        let status = healthy.get_status().await.unwrap();
        server.await.unwrap();
        assert!(matches!(
            error,
            KalshiHttpError::Canceled { dispatched: true }
        ));
        assert_eq!(status, serde_json::from_value(fixture("status")).unwrap());
    }))
    .await
    .unwrap();
}

#[cfg(all(feature = "simulation", madsim))]
#[madsim::test]
async fn concurrent_write_waits_for_healthy_expiry_refresh() {
    Box::pin(time::timeout(Duration::from_secs(90), async {
        let (listener, address) = peer::bind_peer(18220).await;
        let (started, pending) = tokio::sync::oneshot::channel();
        let (resume, resumed) = tokio::sync::oneshot::channel();

        let server = task::spawn(async move {
            quota_discovery(&listener).await;
            let (mut stream, _) = listener.accept().await.unwrap();
            assert!(
                peer::read_headers(&mut stream)
                    .await
                    .contains("/account/limits ")
            );
            started.send(()).unwrap();
            resumed.await.unwrap();
            let body = serde_json::to_string(&fixture("limits")).unwrap();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
            stream.flush().await.unwrap();
            assert!(
                reply(&listener, 200, fixture("costs"))
                    .await
                    .contains("/account/endpoint_costs ")
            );
            assert!(
                reply(&listener, 200, fixture("create"))
                    .await
                    .starts_with("POST ")
            );
        });

        let client = test_client(address);
        client.refresh_quota().await.unwrap();
        time::sleep(Duration::from_secs(60)).await;
        let refresher = client.clone();
        let refresh = task::spawn(async move { refresher.refresh_quota().await });
        pending.await.unwrap();
        let request = create_request();
        let mut write = Box::pin(client.create_order(&request));
        std::future::poll_fn(|cx| {
            assert!(std::future::Future::poll(write.as_mut(), cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;

        resume.send(()).unwrap();
        refresh.await.unwrap().unwrap();
        let response = write.await.unwrap();
        server.await.unwrap();
        assert_eq!(response, serde_json::from_value(fixture("create")).unwrap());
    }))
    .await
    .unwrap();
}

#[cfg(all(feature = "simulation", madsim))]
#[madsim::test]
async fn write_wait_crossing_expiry_refreshes_before_dispatch() {
    Box::pin(time::timeout(Duration::from_secs(90), async {
        let (listener, address) = peer::bind_peer(18221).await;

        let server = task::spawn(async move {
            let mut limits = fixture("limits");
            limits["write"]["refill_rate"] = 1.into();
            limits["write"]["bucket_capacity"] = 11.into();
            assert!(
                reply(&listener, 200, limits)
                    .await
                    .contains("/account/limits ")
            );
            assert!(
                reply(&listener, 200, fixture("costs"))
                    .await
                    .contains("/account/endpoint_costs ")
            );
            assert!(
                reply(&listener, 200, fixture("create"))
                    .await
                    .starts_with("POST ")
            );
            quota_discovery(&listener).await;
            assert!(
                reply(&listener, 200, fixture("create"))
                    .await
                    .starts_with("POST ")
            );
        });

        let client = KalshiHttpClient::new_authenticated(&KalshiDataClientConfig {
            api_key_id: Some("expiry-wait-test".into()),
            private_key: Some(keys::ed25519_pkcs8().into()),
            base_url_http: Some(format!("http://{address}/test").into()),
            http_timeout_secs: 10,
            ..Default::default()
        })
        .unwrap();

        client.refresh_quota().await.unwrap();
        time::sleep(Duration::from_secs(59)).await;
        let request = create_request();
        let first = client.create_order(&request).await.unwrap();
        let second = client.create_order(&request).await.unwrap();
        server.await.unwrap();
        let expected: CreateOrderResponse = serde_json::from_value(fixture("create")).unwrap();
        assert_eq!(first, expected);
        assert_eq!(second, expected);
    }))
    .await
    .unwrap();
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn expired_grant_discovery_enters_bounded_fallback() {
    Box::pin(time::timeout(Duration::from_secs(5), async {
        let (listener, address) = peer::bind_peer(18222).await;
        let (finished, done) = tokio::sync::oneshot::channel();

        let server = task::spawn(async move {
            let mut limits = fixture("limits");
            limits["grants"][0]["expires_ts"] = 0.into();
            assert!(
                reply(&listener, 200, limits)
                    .await
                    .contains("/account/limits ")
            );
            assert!(
                reply(&listener, 200, fixture("costs"))
                    .await
                    .contains("/account/endpoint_costs ")
            );
            assert!(
                reply(&listener, 200, fixture("status"))
                    .await
                    .contains("/exchange/status ")
            );
            tokio::select! {
                biased;
                attempt = listener.accept() => panic!("fallback sent another request: {attempt:?}"),
                () = async { done.await.unwrap(); } => (),
            }
        });

        let client = test_client(address);
        let initial = client.create_order(&create_request()).await.unwrap_err();
        let status = client.get_status().await.unwrap();
        let write = client.create_order(&create_request()).await.unwrap_err();
        finished.send(()).unwrap();
        server.await.unwrap();
        assert!(matches!(initial, KalshiHttpError::Discovery(_)));
        assert!(initial.is_definitely_not_sent());
        assert_eq!(status, serde_json::from_value(fixture("status")).unwrap());
        assert!(matches!(
            write,
            KalshiHttpError::Quota("write quota discovery is unavailable or expired")
        ));
        assert!(write.is_definitely_not_sent());
    }))
    .await
    .unwrap();
}

const DST_TRANSCRIPT: &str = "NAUTILUS_KALSHI_DST_TRANSCRIPT";
const DST_MUTATE: &str = "NAUTILUS_KALSHI_DST_MUTATE";

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn fresh_process_cutoff_transcripts_match() {
    if std::env::var_os(DST_TRANSCRIPT).is_some() {
        return;
    }

    let left = transcript_child(false);
    let right = transcript_child(false);
    let mutated = transcript_child(true);

    assert!(
        left.iter()
            .any(|line| line.contains("GET /custom/v3/historical/cutoff ")),
        "{left:?}"
    );
    assert_eq!(left, right);
    assert_ne!(left, mutated);
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn fresh_process_cutoff_transcript_child() {
    if std::env::var_os(DST_TRANSCRIPT).is_none() {
        return;
    }

    Box::pin(time::timeout(Duration::from_secs(5), async {
        let (listener, address) = peer::bind_peer(0).await;
        let mutate = std::env::var_os(DST_MUTATE).is_some();

        let server = task::spawn(async move {
            let mut body = fixture("cutoff");
            if mutate {
                body["market_settled_ts"] = "2026-10-16T09:09:09.900000010Z".into();
            }

            reply(&listener, 200, body).await
        });

        let client = KalshiHttpClient::new(&KalshiDataClientConfig {
            base_url_http: Some(format!("http://{address}/custom/v3").into()),
            http_timeout_secs: 2,
            ..Default::default()
        })
        .unwrap();

        let cutoff = client.get_historical_cutoff().await.unwrap();
        let headers = server.await.unwrap();
        let request = headers.lines().next().unwrap();
        println!("kalshi-dst-wire: {request}");
        println!(
            "kalshi-dst-domain: {}",
            serde_json::to_string(&cutoff).unwrap()
        );
    }))
    .await
    .unwrap();
}

fn transcript_child(mutate: bool) -> Vec<String> {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "rest::fresh_process_cutoff_transcript_child",
            "--nocapture",
        ])
        .env(DST_TRANSCRIPT, "1")
        .env("MADSIM_TEST_SEED", "7")
        .env("MADSIM_TEST_NUM", "1")
        .env_remove(DST_MUTATE);

    if mutate {
        command.env(DST_MUTATE, "1");
    }

    let output = command.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "transcript child failed\n{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    stdout
        .lines()
        .filter(|line| line.starts_with("kalshi-dst-"))
        .map(str::to_owned)
        .collect()
}

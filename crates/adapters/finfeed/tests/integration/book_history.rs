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

//! Raw historical book selectors, decoding, limits, and cancellation.

use std::{collections::HashMap, sync::Arc, time::Duration};

#[cfg(not(all(feature = "simulation", madsim)))]
use nautilus_finfeed::http::client::FinFeedHttpClient;
use nautilus_finfeed::http::{
    client::FinFeedHttpConfig,
    error::Error,
    models::BookEvent,
    query::{HistoryQuery, PAGE_LIMIT_MAX},
};
#[cfg(not(all(feature = "simulation", madsim)))]
use nautilus_network::retry::RetryConfig;
use nautilus_network::{
    dst::{task, time},
    retry::RetryError,
};
use rstest::rstest;
use rust_decimal::Decimal;
use serde_json::{Value, value::RawValue};

use super::dst::{assert_request, new_client, start_peer};

const EMPTY: &str = include_str!("../../test_data/empty.json");
const EXACT: &str = include_str!("../../test_data/book-history-exact.json");
const ACTIONS: &str = include_str!("../../test_data/book-history-actions.json");
const BATCH: &str = include_str!("../../test_data/book-history-batch.json");

#[rstest]
#[case("date", "date=2026-10-17&limit=2")]
#[case("maximum", "date=2026-10-17&limit=10000")]
#[case(
    "range",
    "time_start=2026-10-17T00%3A00%3A00.0000001Z&time_end=2026-10-18T00%3A00%3A00.0000003Z&limit=2"
)]
#[case("start", "time_start=2026-10-17T00%3A00%3A00.0000001Z&limit=2")]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn selectors_headers_and_every_event_field(#[case] selector: &str, #[case] target: &str) {
    let (peer, config) = start_peer(vec![(200, "", EXACT)]).await;
    let mut query = history(2);
    if selector == "maximum" {
        query.limit = PAGE_LIMIT_MAX;
    }

    if matches!(selector, "range" | "start") {
        query.date = None;
        query.time_start = Some("2026-10-17T00:00:00.0000001Z".parse().unwrap());
    }

    if selector == "range" {
        query.time_end = Some("2026-10-18T00:00:00.0000003Z".parse().unwrap());
    }

    let records = new_client(config.clone())
        .get_book_history("POLY/MARKET", "Outcome/NO?+=%", &query)
        .await
        .unwrap();
    let requests = peer.await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_request(
        &requests[0],
        &format!("/v1/orderbook/POLY%2FMARKET/Outcome%2FNO%3F+=%25/history?{target}"),
        &config,
    );
    assert_eq!(
        records,
        vec![
            BookEvent {
                symbol_id: "MixedCase/Outcome?+=%_NO".into(),
                time_exchange: "2026-10-17T00:00:03.0180674Z".parse().unwrap(),
                time_coinapi: Some("2026-10-17T00:00:04.1234567Z".parse().unwrap()),
                price: "0.1234567890123456789012345678".parse().unwrap(),
                size: "9007199254740993.125".parse().unwrap(),
                is_sell_ask: true,
                update_type: 987,
                id: "opaque/ID?+=%".into(),
            },
            BookEvent {
                symbol_id: "MixedCase/Outcome?+=%_YES".into(),
                time_exchange: "2026-10-17T00:00:03.0180674Z".parse().unwrap(),
                time_coinapi: None,
                price: "0.9876543210987654321098765432".parse().unwrap(),
                size: "0".parse().unwrap(),
                is_sell_ask: false,
                update_type: 1,
                id: String::new(),
            },
        ]
    );
}

#[rstest]
#[case(EXACT, 2)]
#[case(ACTIONS, 6)]
#[case(BATCH, 100)]
#[case(include_str!("../../test_data/empty.json"), 0)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn history_arrays_preserve_order_and_raw_fields(#[case] body: &str, #[case] count: usize) {
    let (peer, config) = start_peer(vec![(200, "", body.to_owned())]).await;
    let records = new_client(config)
        .get_book_history("POLYMARKET", "id", &history(100))
        .await
        .unwrap();
    assert_eq!(records.len(), count);
    assert_raw_records(&records, body);
    assert_eq!(peer.await.unwrap().len(), 1);
}

#[rstest]
#[case("price", "79228162514264337593543950336")]
#[case("size", "1e-29")]
#[case("price", "1e-29")]
#[case("size", "79228162514264337593543950336")]
#[case("time_exchange", "2026-10-17T00:00:03.0180674001Z")]
#[case("time_coinapi", "2026-10-17T00:00:04.1234567001Z")]
#[case("time_exchange", "invalid")]
#[case("update_type", "SET")]
#[case("is_sell_ask", "false")]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn malformed_fields_fail_without_retry(#[case] field: &str, #[case] invalid: &str) {
    let mut body: Value = serde_json::from_str(EXACT).unwrap();
    body[0][field] = invalid.into();
    assert_invalid(body.to_string()).await;
}

#[rstest]
#[case(Value::Null)]
#[case(Value::Bool(true))]
#[case(Value::from(1.5))]
#[case(Value::from(u64::MAX))]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn malformed_actions_fail_without_retry(#[case] action: Value) {
    let mut body: Value = serde_json::from_str(EXACT).unwrap();
    body[0]["update_type"] = action;
    assert_invalid(body.to_string()).await;
}

#[rstest]
#[case("symbol_id")]
#[case("time_exchange")]
#[case("price")]
#[case("size")]
#[case("is_sell_ask")]
#[case("update_type")]
#[case("id")]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn missing_fields_fail_without_retry(#[case] field: &str) {
    let mut body: Value = serde_json::from_str(EXACT).unwrap();
    body[1].as_object_mut().unwrap().remove(field);
    assert_invalid(body.to_string()).await;
}

#[rstest]
#[case(Value::Null)]
#[case(Value::Object(Default::default()))]
#[case(Value::from("invalid"))]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn invalid_envelopes_fail_without_retry(#[case] body: Value) {
    assert_invalid(body.to_string()).await;
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn bounds_validate_before_transmission_and_reject_overfull_arrays() {
    let client = new_client(FinFeedHttpConfig {
        base_url: "http://127.0.0.1:9/v1/".into(),
        ..Default::default()
    });

    let mut queries = vec![history(0), history(PAGE_LIMIT_MAX + 1)];
    let start = "2026-10-17T00:00:00.0000001Z".parse().unwrap();
    let end = "2026-10-18T00:00:00.0000003Z".parse().unwrap();

    for (date, time_start, time_end) in [
        (None, None, None),
        (None, None, Some(end)),
        (history(1).date, Some(start), None),
        (history(1).date, None, Some(end)),
        (None, Some(end), Some(start)),
        (None, Some(start), Some(start)),
        (
            None,
            Some("2026-10-17T00:00:00.000000001Z".parse().unwrap()),
            None,
        ),
        (
            None,
            Some(start),
            Some("2026-10-18T00:00:00.000000001Z".parse().unwrap()),
        ),
    ] {
        queries.push(HistoryQuery {
            date,
            time_start,
            time_end,
            limit: 1,
        });
    }

    for query in queries {
        assert!(matches!(
            client.get_book_history("POLYMARKET", "id", &query).await,
            Err(Error::Validation(_))
        ));
    }

    for (exchange, market) in [("", "id"), ("POLYMARKET", ".."), ("POLYMARKET", " ")] {
        assert!(matches!(
            client.get_book_history(exchange, market, &history(1)).await,
            Err(Error::Validation(_))
        ));
    }

    assert_eq!(client.attempts_remaining(), 100);

    let (peer, config) = start_peer(vec![(200, "", EXACT)]).await;
    let client = new_client(config);
    assert!(matches!(
        client
            .get_book_history("POLYMARKET", "id", &history(1))
            .await,
        Err(Error::Response("response exceeds requested record limit"))
    ));
    assert_eq!(client.attempts_remaining(), 99);
    assert_eq!(peer.await.unwrap().len(), 1);
}

#[rstest]
#[case("trades")]
#[case("quotes")]
#[case("book")]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn subhour_history_is_rejected_before_admission(#[case] endpoint: &str) {
    let client = new_client(FinFeedHttpConfig {
        base_url: "http://127.0.0.1:9/v1/".into(),
        ..Default::default()
    });

    for (start, end) in [
        ("2026-10-11T00:00:00Z", "2026-10-11T00:59:59.9999999Z"),
        ("2026-10-11T00:15:00Z", "2026-10-11T00:45:00Z"),
        ("2026-10-11T01:15:00+01:00", "2026-10-11T00:45:00Z"),
    ] {
        let query = HistoryQuery {
            date: None,
            time_start: Some(start.parse().unwrap()),
            time_end: Some(end.parse().unwrap()),
            limit: 100,
        };

        let error = match endpoint {
            "trades" => client
                .get_trades_history("KALSHI", "id", &query)
                .await
                .unwrap_err(),
            "quotes" => client
                .get_quotes_history("POLYMARKET", "id", &query)
                .await
                .unwrap_err(),
            "book" => client
                .get_book_history("POLYMARKET", "id", &query)
                .await
                .unwrap_err(),
            _ => unreachable!(),
        };

        assert_eq!(
            error.to_string(),
            "Invalid FinFeed request: history time bounds must cross a UTC hour boundary; FinFeed can return empty data for ranges within one hour; use date or covering UTC-hour bounds and filter locally"
        );
        assert_eq!(client.attempts_remaining(), 100);
    }
}

#[rstest]
#[case("trades", "activity/POLYMARKET/id/history/trades")]
#[case("quotes", "activity/POLYMARKET/id/history/quotes")]
#[case("book", "orderbook/POLYMARKET/id/history")]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn hour_boundary_history_preserves_partial_start_bounds(
    #[case] endpoint: &str,
    #[case] path: &str,
) {
    let (peer, config) = start_peer(vec![(200, "", EMPTY)]).await;
    let client = new_client(config.clone());

    let query = HistoryQuery {
        date: None,
        time_start: Some("2026-10-11T00:30:00Z".parse().unwrap()),
        time_end: Some("2026-10-11T01:00:00Z".parse().unwrap()),
        limit: 17,
    };

    let count = match endpoint {
        "trades" => client
            .get_trades_history("POLYMARKET", "id", &query)
            .await
            .unwrap()
            .len(),
        "quotes" => client
            .get_quotes_history("POLYMARKET", "id", &query)
            .await
            .unwrap()
            .len(),
        "book" => client
            .get_book_history("POLYMARKET", "id", &query)
            .await
            .unwrap()
            .len(),
        _ => unreachable!(),
    };

    let requests = peer.await.unwrap();

    assert_eq!(count, 0);
    assert_eq!(client.attempts_remaining(), 99);
    assert_eq!(requests.len(), 1);
    super::dst::assert_request(
        &requests[0],
        &format!(
            "/v1/{path}?time_start=2026-10-11T00%3A30%3A00.0000000Z&time_end=2026-10-11T01%3A00%3A00.0000000Z&limit=17"
        ),
        &config,
    );
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn retries_share_headers_pacing_budget_and_cancellation() {
    let (peer, mut config) = start_peer(vec![
        (429, "Retry-After: 1\r\n", EMPTY),
        (503, "", EMPTY),
        (200, "", EXACT),
    ])
    .await;
    config.min_request_interval_ms = 50;
    config.attempt_budget = 3;
    let client = new_client(config.clone());
    let started = time::Instant::now();
    let records = client
        .get_book_history("POLYMARKET", "id", &history(2))
        .await
        .unwrap();
    let elapsed = started.elapsed();
    assert_eq!(records.len(), 2);
    assert!(elapsed >= Duration::from_millis(1050));
    assert!(matches!(
        client
            .get_book_history("POLYMARKET", "id", &history(2))
            .await,
        Err(Error::AttemptBudget)
    ));
    client.cancel();
    assert!(matches!(
        client
            .get_book_history("POLYMARKET", "id", &history(2))
            .await,
        Err(Error::Retry(RetryError::Canceled))
    ));
    assert_eq!(client.attempts_remaining(), 0);
    let requests = peer.await.unwrap();
    assert_eq!(requests.len(), 3);

    for request in requests {
        assert_request(
            &request,
            "/v1/orderbook/POLYMARKET/id/history?date=2026-10-17&limit=2",
            &config,
        );
    }
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn cancellation_interrupts_history_inflight() {
    let (listener, address) = super::dst::bind_peer().await;

    let client = Arc::new(new_client(FinFeedHttpConfig {
        base_url: format!("http://{address}/v1/"),
        ..Default::default()
    }));

    let pending = task::spawn({
        let client = Arc::clone(&client);
        async move {
            client
                .get_book_history("POLYMARKET", "id", &history(1))
                .await
        }
    });

    let (mut stream, _) = listener.accept().await.unwrap();
    let request = super::dst::read_request(&mut stream).await;
    client.cancel();
    let result = time::timeout(Duration::from_secs(1), pending)
        .await
        .unwrap()
        .unwrap();
    assert!(request.starts_with("GET /v1/orderbook/POLYMARKET/id/history?"));
    assert!(matches!(result, Err(Error::Retry(RetryError::Canceled))));
    assert_eq!(client.attempts_remaining(), 99);
}

#[tokio::test]
#[cfg(not(all(feature = "simulation", madsim)))]
#[ignore = "requires prediction-market credentials and one bounded read-only GET plus a raw sample"]
async fn bounded_live_history_matches_raw_sample() {
    let sample = std::fs::read_to_string(
        std::env::var("FINFEED_BOOK_HISTORY_SAMPLE").expect("raw sample path required"),
    )
    .unwrap();
    let key = std::env::var("FINFEED_API_KEY").expect("prediction-market key required");

    let client = FinFeedHttpClient::new(
        Some(key.into()),
        FinFeedHttpConfig {
            attempt_budget: 1,
            retry: RetryConfig {
                max_retries: 0,
                ..FinFeedHttpConfig::default().retry
            },
            ..Default::default()
        },
    )
    .unwrap();

    let market = "2-FREE-APP-IN-THE-US-APPLE-APP-STORE-ON-OCTOBER-9-20261002T180000000Z-WILL-VINTED-PRE-LOVED-MARKETPL_NO";

    let query = HistoryQuery {
        date: Some("2026-10-08".parse().unwrap()),
        ..history(100)
    };

    let records = client
        .get_book_history("POLYMARKET", market, &query)
        .await
        .unwrap();
    assert_eq!(records.len(), 100);
    assert_eq!(client.attempts_remaining(), 0);
    assert_raw_records(&records, &sample);
    println!(
        "Live FinFeed POLYMARKET history: 100 nonempty records match every modeled field in response order with exact decimals; one managed GET, no retries"
    );
}

fn history(limit: u32) -> HistoryQuery {
    HistoryQuery {
        date: Some("2026-10-17".parse().unwrap()),
        time_start: None,
        time_end: None,
        limit,
    }
}

async fn assert_invalid(body: String) {
    let (peer, config) = start_peer(vec![(200, "", body)]).await;
    let client = new_client(config);
    let result = client
        .get_book_history("POLYMARKET", "id", &history(2))
        .await;
    assert!(matches!(result, Err(Error::Parse { .. })));
    assert_eq!(client.attempts_remaining(), 99);
    assert_eq!(peer.await.unwrap().len(), 1);
}

fn assert_raw_records(records: &[BookEvent], body: &str) {
    let raw: Vec<HashMap<String, Box<RawValue>>> = serde_json::from_str(body).unwrap();
    assert_eq!(records.len(), raw.len());

    for (event, raw) in records.iter().zip(raw) {
        assert_eq!(
            event.symbol_id,
            serde_json::from_str::<String>(raw["symbol_id"].get()).unwrap()
        );
        assert_eq!(
            event.id,
            serde_json::from_str::<String>(raw["id"].get()).unwrap()
        );
        assert_eq!(
            event.update_type,
            serde_json::from_str::<i64>(raw["update_type"].get()).unwrap()
        );
        assert_eq!(
            event.is_sell_ask,
            serde_json::from_str::<bool>(raw["is_sell_ask"].get()).unwrap()
        );
        assert_eq!(event.price, raw["price"].get().parse::<Decimal>().unwrap());
        assert_eq!(event.size, raw["size"].get().parse::<Decimal>().unwrap());
        assert_eq!(
            event.time_exchange,
            serde_json::from_str::<jiff::Timestamp>(raw["time_exchange"].get()).unwrap()
        );
        assert_eq!(
            event.time_coinapi,
            raw.get("time_coinapi").and_then(|value| {
                serde_json::from_str::<Option<jiff::Timestamp>>(value.get()).unwrap()
            })
        );
    }
}

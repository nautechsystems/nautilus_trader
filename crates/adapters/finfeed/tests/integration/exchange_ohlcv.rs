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

//! Exchange-wide OHLCV selectors, raw identifiers, response caps, and cancellation.

use std::{collections::HashMap, sync::Arc, time::Duration};

#[cfg(not(all(feature = "simulation", madsim)))]
use nautilus_finfeed::http::client::FinFeedHttpClient;
use nautilus_finfeed::http::{
    client::FinFeedHttpConfig, error::Error, models::Ohlcv, query::OhlcvQuery,
};
use nautilus_network::{
    dst::{task, time},
    retry::RetryError,
};
use rstest::rstest;
use rust_decimal::Decimal;
use serde_json::{Value, value::RawValue};

use super::dst::{assert_request, new_client, start_peer};

const EXACT: &str = include_str!("../../test_data/exchange-ohlcv-exact.json");

#[rstest]
#[case(2, "1HRS")]
#[case(10_000, "1HRS")]
#[case(2, "45MIN")]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn exchange_route_preserves_market_rows_and_exact_fields(
    #[case] limit: u32,
    #[case] period: &str,
) {
    let (peer, config) = start_peer(vec![(200, "", EXACT)]).await;
    let client = new_client(config.clone());
    let mut query = query(limit);
    query.period_id = period.into();
    let records = client
        .get_ohlcv_exchange_history("MY/RIAD", &query)
        .await
        .unwrap();
    let requests = peer.await.unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(
        records[0],
        Ohlcv {
            market_id_exchange: Some("MixedCase/Outcome?+=%_NO".into()),
            price_open: Some("0.1".parse().unwrap()),
            price_high: Some("0.9".parse().unwrap()),
            price_low: Some("0.01".parse().unwrap()),
            price_close: Some("0.5".parse().unwrap()),
            volume_traded: "9007199254740993.125".parse().unwrap(),
            trades_count: i64::MAX,
            time_period_start: "2026-10-11T00:00:00.0000001Z".parse().unwrap(),
            time_period_end: "2026-10-11T00:01:00.0000002Z".parse().unwrap(),
            time_open: Some("2026-10-11T00:00:02.0000003Z".parse().unwrap()),
            time_close: Some("2026-10-11T00:00:58.0000004Z".parse().unwrap()),
        }
    );
    assert_raw_rows(&records, EXACT);
    assert_eq!(
        records[1].market_id_exchange.as_deref(),
        Some("unknown/YES")
    );
    assert_eq!(client.attempts_remaining(), 99);
    assert_eq!(requests.len(), 1);
    assert_request(
        &requests[0],
        &format!(
            "/v1/ohlcv/MY%2FRIAD/history?period_id={period}&time_start=2026-10-11T00%3A00%3A00.0000001Z&time_end=2026-10-12T00%3A00%3A00.0000001Z&limit={limit}"
        ),
        &config,
    );
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn simultaneous_market_rows_retain_every_market() {
    let mut body: Value = serde_json::from_str(EXACT).unwrap();
    body[1]["time_period_start"] = body[0]["time_period_start"].clone();
    let body = body.to_string();
    let (peer, config) = start_peer(vec![(200, "", body.clone())]).await;
    let records = new_client(config)
        .get_ohlcv_exchange_history("POLYMARKET", &query(2))
        .await
        .unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].time_period_start, records[1].time_period_start);
    assert_raw_rows(&records, &body);
    assert_eq!(peer.await.unwrap().len(), 1);
}

#[rstest]
#[case("missing")]
#[case("null")]
#[case("empty")]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn raw_rows_retain_unmapped_and_absent_identifiers(#[case] kind: &str) {
    let mut raw: Value = serde_json::from_str(EXACT).unwrap();

    match kind {
        "missing" => {
            raw[0].as_object_mut().unwrap().remove("market_id_exchange");
        }
        "null" => raw[0]["market_id_exchange"] = Value::Null,
        "empty" => raw[0]["market_id_exchange"] = "".into(),
        _ => unreachable!(),
    }

    let body = raw.to_string();
    let (peer, config) = start_peer(vec![(200, "", body.clone())]).await;
    let records = new_client(config)
        .get_ohlcv_exchange_history("UNSUPPORTED", &query(2))
        .await
        .unwrap();
    assert_eq!(
        records[0].market_id_exchange.as_deref(),
        (kind == "empty").then_some("")
    );
    assert_raw_rows(&records, &body);
    assert_eq!(peer.await.unwrap().len(), 1);
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn adjacent_explicit_windows_keep_shared_boundary_rows() {
    let (peer, config) = start_peer(vec![(200, "", EXACT), (200, "", EXACT)]).await;
    let client = new_client(config);
    let mut next = query(2);
    next.time_start = next.time_end;
    next.time_end = Some("2026-10-13T00:00:00.0000001Z".parse().unwrap());
    let first = client
        .get_ohlcv_exchange_history("POLYMARKET", &query(2))
        .await
        .unwrap();
    let second = client
        .get_ohlcv_exchange_history("POLYMARKET", &next)
        .await
        .unwrap();
    assert_eq!(first[1].time_period_start, next.time_start.unwrap());
    assert_eq!(first, second);
    assert_eq!(peer.await.unwrap().len(), 2);
}

#[rstest]
#[case("start")]
#[case("end")]
#[case("long")]
#[case("equal")]
#[case("reverse")]
#[case("precision")]
#[case("zero")]
#[case("cap")]
#[case("period")]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn invalid_bounds_fail_before_admission(#[case] kind: &str) {
    let client = new_client(FinFeedHttpConfig {
        base_url: "http://127.0.0.1:9/v1/".into(),
        ..Default::default()
    });

    let mut invalid = query(2);
    match kind {
        "start" => invalid.time_start = None,
        "end" => invalid.time_end = None,
        "long" => invalid.time_end = Some("2026-10-12T00:00:00.0000002Z".parse().unwrap()),
        "equal" => invalid.time_end = invalid.time_start,
        "reverse" => std::mem::swap(&mut invalid.time_start, &mut invalid.time_end),
        "precision" => invalid.time_start = Some("2026-10-11T00:00:00.000000001Z".parse().unwrap()),
        "zero" => invalid.limit = 0,
        "cap" => invalid.limit = 10_001,
        "period" => invalid.period_id = " ".into(),
        _ => unreachable!(),
    }

    let result = client
        .get_ohlcv_exchange_history("POLYMARKET", &invalid)
        .await;

    if matches!(kind, "zero" | "cap") {
        assert!(
            matches!(result, Err(Error::Validation(message)) if message == "limit must be in [1, 10000]")
        );
    } else {
        assert!(matches!(result, Err(Error::Validation(_))));
    }

    assert_eq!(client.attempts_remaining(), 100);
}

#[rstest]
#[case("market_id_exchange", "1")]
#[case("price_open", "1e-29")]
#[case("price_high", "79228162514264337593543950336")]
#[case("volume_traded", "false")]
#[case("trades_count", "1.5")]
#[case("time_period_start", "null")]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn malformed_rows_fail_without_partial_results_or_retry(
    #[case] field: &str,
    #[case] value: &str,
) {
    let mut body: Value = serde_json::from_str(EXACT).unwrap();
    body[1][field] = serde_json::from_str(value).unwrap();
    let (peer, config) = start_peer(vec![(200, "", body.to_string())]).await;
    let client = new_client(config);
    assert!(matches!(
        client
            .get_ohlcv_exchange_history("POLYMARKET", &query(2))
            .await,
        Err(Error::Parse { .. })
    ));
    assert_eq!(client.attempts_remaining(), 99);
    assert_eq!(peer.await.unwrap().len(), 1);
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn response_caps_reject_overflow_and_empty_does_not_continue() {
    let (peer, config) = start_peer(vec![(200, "", EXACT), (200, "", "[]")]).await;
    let client = new_client(config);
    assert!(matches!(
        client
            .get_ohlcv_exchange_history("POLYMARKET", &query(1))
            .await,
        Err(Error::Response("response exceeds requested record limit"))
    ));
    assert_eq!(
        client
            .get_ohlcv_exchange_history("POLYMARKET", &query(2))
            .await
            .unwrap(),
        vec![]
    );
    assert_eq!(client.attempts_remaining(), 98);
    assert_eq!(peer.await.unwrap().len(), 2);
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn cancellation_interrupts_exchange_history_and_future_calls() {
    let (listener, address) = super::dst::bind_peer().await;

    let client = Arc::new(new_client(FinFeedHttpConfig {
        base_url: format!("http://{address}/v1/").into(),
        ..Default::default()
    }));

    let pending = task::spawn({
        let client = Arc::clone(&client);
        async move {
            client
                .get_ohlcv_exchange_history("POLYMARKET", &query(2))
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
    assert!(matches!(result, Err(Error::Retry(RetryError::Canceled))));
    assert!(matches!(
        client
            .get_ohlcv_exchange_history("POLYMARKET", &query(2))
            .await,
        Err(Error::Retry(RetryError::Canceled))
    ));
    assert_eq!(client.attempts_remaining(), 99);
    assert!(request.starts_with("GET /v1/ohlcv/POLYMARKET/history?"));
}

#[tokio::test]
#[cfg(not(all(feature = "simulation", madsim)))]
#[ignore = "requires prediction-market credentials and one bounded GET plus a raw sample"]
async fn bounded_live_exchange_history_matches_raw_sample() {
    let body = std::fs::read_to_string(
        std::env::var("FINFEED_OHLCV_SAMPLE").expect("raw sample required"),
    )
    .unwrap();

    let mut config = FinFeedHttpConfig {
        attempt_budget: 1,
        ..Default::default()
    };

    config.retry.max_retries = 0;
    let client = FinFeedHttpClient::new(None, config).unwrap();

    let query = OhlcvQuery {
        time_start: Some("2026-10-11T00:00:00Z".parse().unwrap()),
        time_end: Some("2026-10-12T00:00:00Z".parse().unwrap()),
        ..query(5)
    };

    let records = client
        .get_ohlcv_exchange_history("POLYMARKET", &query)
        .await
        .unwrap();
    assert_eq!(records.len(), 5);
    assert_eq!(
        records
            .iter()
            .map(|r| r.market_id_exchange.as_ref().unwrap())
            .collect::<std::collections::HashSet<_>>()
            .len(),
        5
    );
    assert_raw_rows(&records, &body);
    assert_eq!(client.attempts_remaining(), 0);
    println!(
        "FinFeed POLYMARKET: five distinct market rows match every raw field; one managed GET, no retries"
    );
}

fn query(limit: u32) -> OhlcvQuery {
    OhlcvQuery {
        period_id: "1HRS".into(),
        time_start: Some("2026-10-11T00:00:00.0000001Z".parse().unwrap()),
        time_end: Some("2026-10-12T00:00:00.0000001Z".parse().unwrap()),
        limit,
    }
}

fn assert_raw_rows(records: &[Ohlcv], body: &str) {
    let raw: Vec<HashMap<String, Box<RawValue>>> = serde_json::from_str(body).unwrap();
    assert_eq!(records.len(), raw.len());

    for (row, raw) in records.iter().zip(raw) {
        assert_eq!(
            row.market_id_exchange,
            raw.get("market_id_exchange")
                .and_then(|v| serde_json::from_str::<Option<ustr::Ustr>>(v.get()).unwrap())
        );
        assert_eq!(
            row.time_period_start,
            serde_json::from_str::<jiff::Timestamp>(raw["time_period_start"].get()).unwrap()
        );
        assert_eq!(
            row.time_period_end,
            serde_json::from_str::<jiff::Timestamp>(raw["time_period_end"].get()).unwrap()
        );
        assert_eq!(
            row.time_open,
            serde_json::from_str::<Option<jiff::Timestamp>>(raw["time_open"].get()).unwrap()
        );
        assert_eq!(
            row.time_close,
            serde_json::from_str::<Option<jiff::Timestamp>>(raw["time_close"].get()).unwrap()
        );

        for (value, field) in [
            (row.price_open, "price_open"),
            (row.price_high, "price_high"),
            (row.price_low, "price_low"),
            (row.price_close, "price_close"),
        ] {
            let token = raw[field].get();

            let expected = if token == "null" {
                None
            } else {
                Some(token.trim_matches('"').parse::<Decimal>().unwrap())
            };

            assert_eq!(value, expected, "{field}");
        }

        assert_eq!(
            row.volume_traded,
            raw["volume_traded"]
                .get()
                .trim_matches('"')
                .parse::<Decimal>()
                .unwrap()
        );
        assert_eq!(
            row.trades_count,
            raw["trades_count"].get().parse::<i64>().unwrap()
        );
    }
}

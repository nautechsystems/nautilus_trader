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

//! Flat-file OHLCV selectors and separation from DB-backed bars.

use nautilus_finfeed::http::{
    client::FinFeedHttpConfig,
    error::Error,
    models::Ohlcv,
    query::{FlatFileOhlcvQuery, HistoryQuery, OhlcvQuery},
};
use rstest::rstest;

use super::dst::{assert_request, new_client, start_peer};

const EXACT: &str = include_str!("../../test_data/exact-bar.json");

#[rstest]
#[case("1SEC", "date", 1)]
#[case("1MIN", "range", 10_000)]
#[case("1HRS", "date", 3)]
#[case("1MIN", "start", 2)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn flat_file_route_preserves_fields_and_selectors(
    #[case] period: &str,
    #[case] selector_kind: &str,
    #[case] limit: u32,
) {
    let body = format!("[{EXACT}]");
    let (peer, config) = start_peer(vec![(200, "", body)]).await;
    let client = new_client(config.clone());
    let mut query = query(period, limit);
    let selector = if selector_kind == "start" {
        query.history.date = None;
        query.history.time_start = Some("2026-10-11T00:00:00.0000001Z".parse().unwrap());
        "time_start=2026-10-11T00%3A00%3A00.0000001Z"
    } else if selector_kind == "range" {
        query.history.date = None;
        query.history.time_start = Some("2026-10-11T00:00:00.0000001Z".parse().unwrap());
        query.history.time_end = Some("2026-10-11T01:00:00.0000002Z".parse().unwrap());
        "time_start=2026-10-11T00%3A00%3A00.0000001Z&time_end=2026-10-11T01%3A00%3A00.0000002Z"
    } else {
        "date=2026-10-11"
    };

    let rows = client
        .get_ohlcv_flat_file_history("KAL/SHI", "Mixed?NO", &query)
        .await
        .unwrap();
    let requests = peer.await.unwrap();
    assert_eq!(
        rows,
        vec![Ohlcv {
            market_id_exchange: None,
            time_period_start: "2026-10-11T00:00:00.0000001Z".parse().unwrap(),
            time_period_end: "2026-10-11T00:01:00.0000002Z".parse().unwrap(),
            time_open: Some("2026-10-11T00:00:02.0000003Z".parse().unwrap()),
            time_close: Some("2026-10-11T00:00:58.0000004Z".parse().unwrap()),
            price_open: Some("0.1".parse().unwrap()),
            price_high: Some("0.9".parse().unwrap()),
            price_low: Some("0.01".parse().unwrap()),
            price_close: Some("0.5".parse().unwrap()),
            volume_traded: "9007199254740993.125".parse().unwrap(),
            trades_count: i64::MAX,
        }]
    );
    assert_eq!(requests.len(), 1);
    assert_request(
        &requests[0],
        &format!(
            "/v1/activity/KAL%2FSHI/Mixed%3FNO/history/ohlcv?period={period}&{selector}&limit={limit}"
        ),
        &config,
    );
    assert_eq!(client.attempts_remaining(), 99);
}

#[rstest]
#[case("period", "flat-file period must be 1SEC, 1MIN, or 1HRS")]
#[case("missing", "date or time_start is required")]
#[case("both", "date and time bounds are mutually exclusive")]
#[case("end", "date or time_start is required")]
#[case("equal", "time_start must precede time_end")]
#[case("precision", "timestamps must fit the provider's 100 ns precision")]
#[case("zero", "limit must be in [1, 10000]")]
#[case("cap", "limit must be in [1, 10000]")]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn flat_file_validation_precedes_admission(#[case] kind: &str, #[case] message: &str) {
    let client = new_client(FinFeedHttpConfig {
        base_url: "http://127.0.0.1:9/v1/".into(),
        ..Default::default()
    });

    let mut query = query("1MIN", 2);
    match kind {
        "period" => query.period = "1DAY".into(),
        "missing" => query.history.date = None,
        "both" => query.history.time_end = Some("2026-10-11T01:00:00Z".parse().unwrap()),
        "end" => {
            query.history.date = None;
            query.history.time_end = Some("2026-10-11T01:00:00Z".parse().unwrap());
        }
        "equal" | "precision" => {
            query.history.date = None;
            query.history.time_start = Some("2026-10-11T00:00:00Z".parse().unwrap());
            query.history.time_end = Some(
                if kind == "equal" {
                    "2026-10-11T00:00:00Z"
                } else {
                    "2026-10-11T01:00:00.000000001Z"
                }
                .parse()
                .unwrap(),
            );
        }
        "zero" => query.history.limit = 0,
        "cap" => query.history.limit = 10_001,
        _ => unreachable!(),
    }

    assert!(
        matches!(client.get_ohlcv_flat_file_history("KALSHI", "m", &query).await, Err(Error::Validation(value)) if value == message)
    );
    assert_eq!(client.attempts_remaining(), 100);
}

#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn flat_file_and_database_sources_remain_distinct() {
    let (peer, config) = start_peer(vec![
        (200, "", format!("[{EXACT}]")),
        (200, "", "[]".into()),
    ])
    .await;
    let client = new_client(config);
    let flat = client
        .get_ohlcv_flat_file_history("KALSHI", "m", &query("1MIN", 2))
        .await
        .unwrap();

    let db = client
        .get_ohlcv_history(
            "KALSHI",
            "m",
            &OhlcvQuery {
                period_id: "1MIN".into(),
                time_start: None,
                time_end: None,
                limit: 2,
            },
        )
        .await
        .unwrap();

    let requests = peer.await.unwrap();
    assert_eq!(flat.len(), 1);
    assert!(db.is_empty());
    assert!(requests[0].starts_with(
        "GET /v1/activity/KALSHI/m/history/ohlcv?period=1MIN&date=2026-10-11&limit=2 "
    ));
    assert!(requests[1].starts_with("GET /v1/ohlcv/KALSHI/m/history?period_id=1MIN&limit=2 "));
    assert_eq!(requests.len(), 2);
}

#[rstest]
#[case(404, "[]", "FinFeed HTTP status 404")]
#[case(400, "[]", "FinFeed HTTP status 400")]
#[case(200, "[]", "")]
#[case(
    200,
    "overflow",
    "Unexpected FinFeed response: response exceeds requested record limit"
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
#[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
async fn flat_file_failure_and_empty_never_fall_back(
    #[case] status: u16,
    #[case] body: &str,
    #[case] error: &str,
) {
    let body = if body == "overflow" {
        format!("[{EXACT},{EXACT}]")
    } else {
        body.into()
    };

    let (peer, config) = start_peer(vec![(status, "", body)]).await;
    let client = new_client(config);
    let result = client
        .get_ohlcv_flat_file_history("KALSHI", "m", &query("1MIN", 1))
        .await;

    if error.is_empty() {
        assert!(result.unwrap().is_empty());
    } else {
        assert_eq!(result.unwrap_err().to_string(), error);
    }

    assert_eq!(peer.await.unwrap().len(), 1);
    assert_eq!(client.attempts_remaining(), 99);
}

fn query(period: &str, limit: u32) -> FlatFileOhlcvQuery {
    FlatFileOhlcvQuery {
        period: period.into(),
        history: HistoryQuery {
            date: Some("2026-10-11".parse().unwrap()),
            time_start: None,
            time_end: None,
            limit,
        },
    }
}

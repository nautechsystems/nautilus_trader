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

//! Opt-in live comparisons of historical quotes and completed-bar delivery.

#![cfg(not(all(feature = "simulation", madsim)))]

use std::{
    io::{Read, Write},
    num::NonZeroUsize,
    process::{Command, Stdio},
    time::Duration,
};

use nautilus_common::{
    clients::DataClient,
    logging::{
        logger::{LogGuard, LoggerConfig},
        writer::FileWriterConfig,
    },
    messages::{DataEvent, DataResponse, data::*},
};
use nautilus_core::{
    UUID4, UnixNanos, string::secret::SecretString, time::get_atomic_clock_realtime,
};
use nautilus_finfeed::{
    common::symbol::to_instrument_id,
    config::FinFeedDataClientConfig,
    data::FinFeedDataClient,
    http::{
        client::{FinFeedHttpClient, FinFeedHttpConfig},
        models::{Ohlcv, Quote},
        query::HistoryQuery,
    },
    instruments::FinFeedInstrumentSelection,
};
use nautilus_model::{
    data::{Bar, BarType, Data, QuoteTick},
    identifiers::{ClientId, TraderId},
    instruments::Instrument,
    types::{Price, Quantity},
};
use nautilus_network::retry::RetryConfig;
use rstest::rstest;
use rust_decimal::{Decimal, RoundingStrategy};
use tokio::io::AsyncWriteExt;

use super::dst::{assert_request, bind_peer, read_request, start_peer};

const QUOTE_MARKET: &str = "100KT-METEOR-STRIKE-IN-2026_NO";
const QUOTE_DATE: &str = "2026-10-28";
const BAR_ATTEMPTS: u64 = 9;
const BAR_MARKET: &str = "2026-BALANCE-OF-POWER-D-SENATE-D-HOUSE-949_NO";

#[tokio::test]
#[ignore = "requires scoped credential stdin; at most four serial production GETs, no retries"]
async fn bounded_quote_history_matches_received_provider_rows() {
    let _logging = logging();
    let market = QUOTE_MARKET;
    let path =
        format!("/v1/activity/POLYMARKET/{market}/history/quotes?date={QUOTE_DATE}&limit=10000");
    let (control_path, query) = quote_control();
    let start = query.time_start.unwrap();
    let end = query.time_end.unwrap();
    let (mut http, mut captures, peer) =
        recording_peer(market, vec![path.clone(), control_path], 4, 600).await;

    let control = FinFeedHttpClient::new(
        Some(SecretString::from("recording-peer")),
        FinFeedHttpConfig {
            attempt_budget: 1,
            ..http.clone()
        },
    )
    .unwrap();

    let control = control
        .get_quotes_history("POLYMARKET", market, &query)
        .await
        .unwrap();

    assert!(
        !control.is_empty(),
        "source unavailable: covering-hour control returned no quotes"
    );
    println!(
        "Positive covering-hour control: {} raw quotes in [{start}, {end}]",
        control.len()
    );
    http.attempt_budget = 3;
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut client = data_client(market, http, sender);
    client.connect().await.unwrap();

    let DataEvent::Data(Data::Instrument(instrument)) = receiver.try_recv().unwrap() else {
        panic!("expected bootstrap definition")
    };

    let id = to_instrument_id("POLYMARKET", market).unwrap();
    assert_eq!(instrument.id(), id);
    let correlation_id = UUID4::new();
    let requested_client = ClientId::from("QUOTE-CONTROL");
    let before = get_atomic_clock_realtime().get_time_ns();
    client
        .request_quotes(RequestQuotes::new(
            id,
            Some(start),
            Some(end),
            NonZeroUsize::new(3),
            Some(requested_client),
            correlation_id,
            UnixNanos::from(17),
            None,
        ))
        .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(640), receiver.recv()).await;
    client.disconnect().await.unwrap();
    let captured = finish_captures(&mut captures, peer).await;
    let source = captured
        .iter()
        .find(|capture| capture.path == path)
        .expect("quote request did not reach provider admission");
    assert_eq!(
        source.status,
        Some(200),
        "quote retrieval failed before conversion"
    );
    let raw: Vec<Quote> = serde_json::from_slice(&source.body).unwrap();
    assert!(
        !raw.is_empty(),
        "source unavailable: empty whole-date quote response"
    );
    assert!(
        raw.len() < 10_000,
        "source saturated: native conversion must reject this date"
    );
    let mut selected: Vec<_> = raw
        .iter()
        .filter(|row| row.recv_time >= start && row.recv_time <= end)
        .collect();
    selected.sort_by_key(|row| row.recv_time);
    selected.truncate(3);
    assert_eq!(
        selected.len(),
        3,
        "source unavailable: requested window has fewer than three quotes"
    );

    let expected: Vec<_> = selected
        .iter()
        .map(|row| {
            QuoteTick::new_checked(
                id,
                Price::from_decimal_dp(row.bid, instrument.price_precision()).unwrap(),
                Price::from_decimal_dp(row.ask, instrument.price_precision()).unwrap(),
                Quantity::from_decimal_dp(row.bid_volume, instrument.size_precision()).unwrap(),
                Quantity::from_decimal_dp(row.ask_volume, instrument.size_precision()).unwrap(),
                nanos(row.entry_time),
                nanos(row.recv_time),
            )
            .unwrap()
        })
        .collect();

    let DataEvent::Response(DataResponse::Quotes(response)) = result
        .expect("native quote delivery deadline expired; retain request error-chain evidence")
        .expect("data channel closed")
    else {
        panic!("expected quote response")
    };

    assert_eq!(response.correlation_id, correlation_id);
    assert_eq!(response.client_id, requested_client);
    assert_eq!(response.instrument_ids, vec![id]);
    assert_eq!(response.start, Some(nanos(start)));
    assert_eq!(response.end, Some(nanos(end)));
    assert_eq!(response.params, None);
    assert!(
        response.ts_init >= before && response.ts_init <= get_atomic_clock_realtime().get_time_ns()
    );
    assert_eq!(&*response.data, expected.as_slice());

    for (actual, row) in response.data.iter().zip(selected) {
        assert_eq!(actual.bid_price.as_decimal(), row.bid);
        assert_eq!(actual.ask_price.as_decimal(), row.ask);
        assert_eq!(actual.bid_size.as_decimal(), row.bid_volume);
        assert_eq!(actual.ask_size.as_decimal(), row.ask_volume);
    }

    assert!(receiver.try_recv().is_err());
    println!(
        "Native quotes: {} raw date rows, three exact delivered ticks in [{start}, {end}], four GETs including control, no completeness claim",
        raw.len()
    );
}

#[tokio::test]
#[ignore = "requires scoped credential stdin; at most eighteen serial production GETs, no retries"]
async fn bounded_closed_bar_publication_and_reconnect() {
    let _logging = logging();
    let market = BAR_MARKET;
    let path = format!("/v1/ohlcv/POLYMARKET/{market}/latest?period_id=1SEC&limit=101");
    let (mut http, mut captures, peer) = recording_peer(
        market,
        vec!["/v1/ohlcv/periods".into(), path.clone()],
        (BAR_ATTEMPTS * 2) as usize,
        30,
    )
    .await;
    http.attempt_budget = BAR_ATTEMPTS;
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut client = data_client(market, http, sender);
    client.connect().await.unwrap();

    let DataEvent::Data(Data::Instrument(instrument)) = receiver.try_recv().unwrap() else {
        panic!("expected bootstrap definition")
    };

    let id = to_instrument_id("POLYMARKET", market).unwrap();
    assert_eq!(instrument.id(), id);
    let bar_type = BarType::from(format!("{id}-1-SECOND-LAST-EXTERNAL").as_str());
    let subscribed = get_atomic_clock_realtime().get_time_ns();
    client
        .subscribe_bars(SubscribeBars::new(
            bar_type,
            Some(client.client_id()),
            None,
            UUID4::new(),
            subscribed,
            None,
            None,
        ))
        .unwrap();
    let first = receive_bar(&mut receiver).await;
    client.disconnect().await.unwrap();
    let mut initial = vec![first.expect("no eligible completed bar published before disconnect")];
    drain_bars(&mut receiver, &mut initial);
    let retained = initial.last().unwrap().ts_event;
    let mut initial_captures = Vec::new();
    while let Ok(capture) = captures.try_recv() {
        initial_captures.push(capture);
    }

    assert_bars(
        &initial,
        &initial_captures,
        &path,
        bar_type,
        subscribed,
        instrument.price_precision(),
        instrument.size_precision(),
    );
    let reconnected = get_atomic_clock_realtime().get_time_ns();
    client.connect().await.unwrap();

    let DataEvent::Data(Data::Instrument(restored)) = receiver.try_recv().unwrap() else {
        panic!("expected reconnect definition")
    };

    assert_eq!(restored.id(), id);
    let resumed = receive_bar(&mut receiver).await;
    client.disconnect().await.unwrap();
    let mut resumed = vec![resumed.expect("no positive resumed completed-bar delivery")];
    drain_bars(&mut receiver, &mut resumed);
    let restored_captures = finish_captures(&mut captures, peer).await;
    assert!(
        restored_captures
            .iter()
            .all(|capture| capture.started >= reconnected)
    );
    assert!(
        resumed[0].ts_event <= reconnected,
        "source unavailable: resumed delivery cannot distinguish a watermark reset to reconnect time"
    );
    assert_bars(
        &resumed,
        &restored_captures,
        &path,
        bar_type,
        retained,
        instrument.price_precision(),
        instrument.size_precision(),
    );
    assert!(receiver.try_recv().is_err());
    println!(
        "Completed bars: {} initial, {} resumed, subscribe={subscribed}, retained={retained}, reconnect={reconnected}; exact returned rows, positive reconnect advancement, no finality or continuity claim",
        initial.len(),
        resumed.len()
    );
}

#[tokio::test]
async fn covering_hour_control_matches_admitted_wire_selector() {
    let (peer, http) = start_peer(vec![(
        200,
        "",
        include_str!("../../test_data/client-quotes.json"),
    )])
    .await;
    let client =
        FinFeedHttpClient::new(Some(SecretString::from("test-api-key")), http.clone()).unwrap();
    let (path, query) = quote_control();
    client
        .get_quotes_history("POLYMARKET", QUOTE_MARKET, &query)
        .await
        .unwrap();
    let requests = peer.await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_request(&requests[0], &path, &http);
}

fn quote_control() -> (String, HistoryQuery) {
    let path = format!(
        "/v1/activity/POLYMARKET/{QUOTE_MARKET}/history/quotes?time_start={QUOTE_DATE}T00%3A00%3A00.0000000Z&time_end={QUOTE_DATE}T01%3A00%3A00.0000000Z&limit=100"
    );

    let query = HistoryQuery {
        date: None,
        time_start: Some(format!("{QUOTE_DATE}T00:00:00Z").parse().unwrap()),
        time_end: Some(format!("{QUOTE_DATE}T01:00:00Z").parse().unwrap()),
        limit: 100,
    };

    (path, query)
}

async fn receive_bar(
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
) -> Option<Bar> {
    match tokio::time::timeout(Duration::from_secs(600), receiver.recv()).await {
        Ok(Some(DataEvent::Data(Data::Bar(bar)))) => Some(bar),
        Err(_) => None,
        event => panic!("expected completed bar, received {event:?}"),
    }
}

fn drain_bars(receiver: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>, bars: &mut Vec<Bar>) {
    while let Ok(event) = receiver.try_recv() {
        let DataEvent::Data(Data::Bar(bar)) = event else {
            panic!("unexpected event")
        };

        bars.push(bar);
    }
}

fn assert_bars(
    bars: &[Bar],
    captures: &[Capture],
    path: &str,
    bar_type: BarType,
    mut watermark: UnixNanos,
    price_precision: u8,
    size_precision: u8,
) {
    assert!(!bars.is_empty(), "positive completed-bar delivery required");
    let mut rows = Vec::new();

    for capture in captures.iter().filter(|capture| capture.path == path) {
        assert_eq!(
            capture.status,
            Some(200),
            "bar retrieval failed before conversion"
        );
        let raw: Vec<Ohlcv> = serde_json::from_slice(&capture.body).unwrap();
        println!(
            "Bar source: {} rows, request={}, period ends={:?}",
            raw.len(),
            capture.started,
            raw.iter()
                .map(|row| row.time_period_end)
                .collect::<Vec<_>>()
        );
        rows.extend(raw.into_iter().map(|row| (capture.started, row)));
    }

    for bar in bars {
        assert!(
            bar.ts_event > watermark,
            "replayed or unordered completed bar"
        );

        assert_eq!(bar.bar_type, bar_type);
        assert_eq!(bar.ts_init, bar.ts_event);
        assert_eq!(bar.volume.precision, size_precision);

        for price in [bar.open, bar.high, bar.low, bar.close] {
            assert_eq!(price.precision, price_precision);
        }

        assert!(
            rows.iter().any(|(started, row)| {
                let rounded = row.volume_traded.round_dp_with_strategy(
                    u32::from(size_precision),
                    RoundingStrategy::MidpointNearestEven,
                );
                row.time_period_end.as_nanosecond() == i128::from(bar.ts_event.as_u64())
                    && bar.ts_event <= *started
                    && row.time_period_end.as_nanosecond() - row.time_period_start.as_nanosecond()
                        == 1_000_000_000
                    && [
                        (bar.open, row.price_open),
                        (bar.high, row.price_high),
                        (bar.low, row.price_low),
                        (bar.close, row.price_close),
                    ]
                    .iter()
                    .all(|(actual, expected)| Some(actual.as_decimal()) == *expected)
                    && bar.volume.as_decimal() == rounded
                    && (rounded == row.volume_traded
                        || (row.volume_traded < Decimal::from(500_000_000)
                            && (rounded - row.volume_traded).abs()
                                <= row.volume_traded * Decimal::new(1, 15)))
            }),
            "published bar has no matching completed returned provider period: {bar:?}"
        );

        watermark = bar.ts_event;
    }
}

fn nanos(timestamp: jiff::Timestamp) -> UnixNanos {
    UnixNanos::from(u64::try_from(timestamp.as_nanosecond()).unwrap())
}

#[rstest]
#[case("valid", false)]
#[case("empty", true)]
#[case("replay", true)]
#[case("open", true)]
#[case("high", true)]
#[case("low", true)]
#[case("close", true)]
#[case("volume", true)]
#[case("timestamp", true)]
#[case("identity", true)]
#[case("future", true)]
#[case("empty-then-priced", false)]
#[case("revised-then-matching", false)]
#[case("empty-then-corrupt", true)]
fn bar_comparison_rejects_missing_replayed_or_corrupt_delivery(
    #[case] scenario: &str,
    #[case] rejects: bool,
) {
    let mut rows: serde_json::Value =
        serde_json::from_str(include_str!("../../test_data/client-bars.json")).unwrap();
    let row = &mut rows.as_array_mut().unwrap()[0];
    row["time_period_start"] = "2020-10-11T00:01:59Z".into();
    let body = serde_json::to_vec(&vec![row]).unwrap();
    let id = to_instrument_id("POLYMARKET", BAR_MARKET).unwrap();
    let bar_type = BarType::from(format!("{id}-1-SECOND-LAST-EXTERNAL").as_str());
    let timestamp: UnixNanos = "2020-10-11T00:02:00Z".parse().unwrap();

    let mut bar = Bar::new(
        bar_type,
        Price::from("0.2000"),
        Price::from("0.4000"),
        Price::from("0.1000"),
        Price::from("0.3000"),
        Quantity::from("12.345678"),
        timestamp,
        timestamp,
    );
    let mut watermark = UnixNanos::from(timestamp.as_u64() - 1);

    let mut capture = Capture {
        path: "source".into(),
        started: timestamp,
        status: Some(200),
        body,
    };

    match scenario {
        "replay" => watermark = timestamp,
        "open" => bar.open = Price::from("0.2100"),
        "high" => bar.high = Price::from("0.4100"),
        "low" => bar.low = Price::from("0.1100"),
        "close" | "empty-then-corrupt" => bar.close = Price::from("0.3100"),
        "volume" => bar.volume = Quantity::from("12.345679"),
        "timestamp" => bar.ts_init = UnixNanos::from(timestamp.as_u64() + 1),
        "identity" => bar.bar_type = BarType::from(format!("{id}-1-MINUTE-LAST-EXTERNAL").as_str()),
        "future" => capture.started = watermark,
        "valid" | "empty" | "empty-then-priced" | "revised-then-matching" => {}
        _ => unreachable!(),
    }

    let bars = if scenario == "empty" {
        vec![]
    } else {
        vec![bar]
    };

    let captures = if matches!(
        scenario,
        "empty-then-priced" | "revised-then-matching" | "empty-then-corrupt"
    ) {
        let mut earlier: serde_json::Value = serde_json::from_slice(&capture.body).unwrap();
        if scenario == "revised-then-matching" {
            earlier[0]["price_close"] = "0.3100".into();
        } else {
            for field in ["price_open", "price_high", "price_low", "price_close"] {
                earlier[0][field] = serde_json::Value::Null;
            }

            earlier[0]["volume_traded"] = "0".into();
            earlier[0]["trades_count"] = 0.into();
        }

        let earlier = Capture {
            path: capture.path.clone(),
            started: timestamp,
            status: Some(200),
            body: serde_json::to_vec(&earlier).unwrap(),
        };

        capture.started = UnixNanos::from(timestamp.as_u64() + 1);
        vec![earlier, capture]
    } else {
        vec![capture]
    };

    let result = std::panic::catch_unwind(|| {
        assert_bars(&bars, &captures, "source", bar_type, watermark, 4, 6);
    });

    assert_eq!(result.is_err(), rejects);
}

fn data_client(
    market: &str,
    http: FinFeedHttpConfig,
    sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
) -> FinFeedDataClient {
    let config = FinFeedDataClientConfig {
        api_key: Some(SecretString::from("recording-peer")),
        http,
        instruments: vec![FinFeedInstrumentSelection {
            exchange_id: "POLYMARKET".into(),
            market_ids: vec![market.into()],
        }],
        connect_timeout_secs: 240,
        poll_interval_ms: 100_000,
        ..Default::default()
    };

    FinFeedDataClient::new(ClientId::from("FINFEED"), config, sender.into()).unwrap()
}

struct Capture {
    path: String,
    started: UnixNanos,
    status: Option<u16>,
    body: Vec<u8>,
}

async fn recording_peer(
    market: &str,
    mut paths: Vec<String>,
    budget: usize,
    timeout: u64,
) -> (
    FinFeedHttpConfig,
    tokio::sync::mpsc::UnboundedReceiver<Capture>,
    tokio::task::JoinHandle<()>,
) {
    paths.extend([
        "/v1/exchanges".into(),
        format!("/v1/markets/POLYMARKET/{market}"),
    ]);
    let key = credential();
    let (listener, address) = bind_peer().await;
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();

    let peer = tokio::spawn(async move {
        let mut next = tokio::time::Instant::now();

        for attempt in 1..=budget {
            let Ok(Ok((mut stream, _))) =
                tokio::time::timeout(Duration::from_secs(240), listener.accept()).await
            else {
                return;
            };

            let request = read_request(&mut stream).await;
            let path = request
                .lines()
                .next()
                .unwrap()
                .strip_prefix("GET ")
                .unwrap()
                .strip_suffix(" HTTP/1.1")
                .unwrap();
            assert!(
                paths.iter().any(|allowed| allowed == path),
                "unexpected live GET path: {path}"
            );
            tokio::time::sleep_until(next).await;
            let started = get_atomic_clock_realtime().get_time_ns();
            next = tokio::time::Instant::now() + Duration::from_secs(10);
            println!("Production GET {attempt}/{budget}: {path}, start={started}");
            let url = format!("https://api.prediction-markets.finfeedapi.com{path}");
            let key = key.clone();
            let (status, body) =
                tokio::task::spawn_blocking(move || forward_get(&url, timeout, &key))
                    .await
                    .unwrap();
            println!(
                "Provider GET {attempt}: status={status:?}, bytes={}",
                body.len()
            );

            if path.starts_with("/v1/ohlcv/POLYMARKET/") && status == Some(200) {
                let rows: Vec<Ohlcv> =
                    serde_json::from_slice(&body).expect("invalid raw OHLCV response");
                println!(
                    "Returned period ends: {:?}",
                    rows.iter()
                        .map(|row| row.time_period_end)
                        .collect::<Vec<_>>()
                );
            }

            sender
                .send(Capture {
                    path: path.to_owned(),
                    started,
                    status,
                    body: body.clone(),
                })
                .unwrap();

            let header = format!(
                "HTTP/1.1 {} Captured\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                status.unwrap_or(504),
                body.len()
            );
            let _ = stream.write_all(header.as_bytes()).await;
            let _ = stream.write_all(&body).await;
            let _ = stream.flush().await;

            if status != Some(200) {
                return;
            }
        }
    });

    let config = FinFeedHttpConfig {
        base_url: format!("http://{address}/v1/").into(),
        attempt_budget: budget as u64,
        timeout_secs: timeout + 15,
        retry: RetryConfig {
            max_retries: 0,
            jitter_ms: 0,
            operation_timeout_ms: Some((timeout + 20) * 1_000),
            max_elapsed_ms: Some((timeout + 20) * 1_000),
            ..Default::default()
        },
        ..Default::default()
    };

    (config, receiver, peer)
}

fn forward_get(url: &str, timeout: u64, key: &SecretString) -> (Option<u16>, Vec<u8>) {
    let mut child = Command::new("curl")
        .args([
            "--disable",
            "--silent",
            "--http1.1",
            "--retry",
            "0",
            "--max-redirs",
            "0",
            "--noproxy",
            "*",
            "--proto",
            "=https",
            "--config",
            "-",
            "--max-time",
            &timeout.to_string(),
            "--url",
            url,
            "--write-out",
            "\n%{http_code}",
        ])
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("trusted curl executable required");
    let config = format!(
        "header = \"Authorization: {}\"\nheader = \"X-Referrer: nautilus\"\nheader = \"Accept: application/json\"\n",
        key.expose_secret()
    );
    child
        .stdin
        .take()
        .unwrap()
        .write_all(config.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    if !output.status.success() {
        println!(
            "Provider transport/deadline failure: curl exit={:?}, deadline={timeout}s, no provider HTTP status established",
            output.status.code()
        );
        return (None, Vec::new());
    }

    let split = output
        .stdout
        .iter()
        .rposition(|byte| *byte == b'\n')
        .expect("curl status suffix required");
    let status = std::str::from_utf8(&output.stdout[split + 1..])
        .unwrap()
        .parse()
        .unwrap();
    (Some(status), output.stdout[..split].to_vec())
}

async fn finish_captures(
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<Capture>,
    peer: tokio::task::JoinHandle<()>,
) -> Vec<Capture> {
    peer.abort();
    if let Err(e) = peer.await {
        assert!(e.is_cancelled(), "recording peer failed: {e}");
    }

    let mut captures = Vec::new();
    while let Ok(capture) = receiver.try_recv() {
        captures.push(capture);
    }

    captures
}

fn credential() -> SecretString {
    let mut input = String::new();
    std::io::stdin()
        .take(4096)
        .read_to_string(&mut input)
        .unwrap();
    parse_credential(&input)
        .expect("invalid scoped credential input or missing extraction success signal")
}

fn parse_credential(input: &str) -> Result<SecretString, &'static str> {
    let key = input
        .strip_suffix("\nSOPS_OK\n")
        .ok_or("missing extraction success signal")?;

    if key.is_empty()
        || !key
            .bytes()
            .all(|value| value.is_ascii_alphanumeric() || value == b'-' || value == b'_')
    {
        return Err("invalid credential format");
    }

    Ok(SecretString::from(key.to_owned()))
}

#[rstest]
#[case("synthetic-key\nSOPS_OK\n", true)]
#[case("synthetic-key", false)]
#[case("\nSOPS_OK\n", false)]
#[case("synthetic\nkey\nSOPS_OK\n", false)]
#[case("synthetic\rkey\nSOPS_OK\n", false)]
fn scoped_credential_requires_success_and_valid_input(#[case] input: &str, #[case] valid: bool) {
    let result = parse_credential(input);
    assert_eq!(result.is_ok(), valid);

    if valid {
        assert_eq!(result.unwrap().expose_secret(), "synthetic-key");
    }
}

fn logging() -> LogGuard {
    nautilus_common::logging::init_logging(
        TraderId::from("TRADER-001"),
        UUID4::new(),
        LoggerConfig {
            is_colored: false,
            module_level: [("nautilus_finfeed".into(), log::LevelFilter::Debug)]
                .into_iter()
                .collect(),
            ..Default::default()
        },
        FileWriterConfig::default(),
    )
    .unwrap()
}

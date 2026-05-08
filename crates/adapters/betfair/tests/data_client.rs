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

//! Integration tests for `BetfairDataClient`.

mod common;

use std::{net::SocketAddr, sync::Arc, time::Duration};

use nautilus_betfair::{
    common::consts::{BETFAIR_CLIENT_ID, BETFAIR_VENUE},
    config::BetfairDataConfig,
    data::BetfairDataClient,
    provider::NavigationFilter,
};
use nautilus_common::{
    clients::DataClient,
    live::runner::set_data_event_sender,
    messages::{DataEvent, data::SubscribeBookDeltas},
    testing::wait_until_async,
};
use nautilus_core::UUID4;
use nautilus_model::{
    data::Data,
    enums::{BookType, MarketStatusAction},
    types::Currency,
};
use rstest::rstest;
use serde_json::Value;

use crate::common::*;

fn create_test_data_client(
    addr: SocketAddr,
    stream_port: u16,
) -> (
    BetfairDataClient,
    tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    set_data_event_sender(tx);

    let http_client = create_test_http_client(addr);
    let currency = Currency::GBP();

    let client = BetfairDataClient::new(
        *BETFAIR_CLIENT_ID,
        http_client,
        test_credential(),
        plain_stream_config(stream_port),
        BetfairDataConfig::default(),
        NavigationFilter::default(),
        currency,
        None,
    );

    (client, rx)
}

#[rstest]
#[tokio::test]
async fn test_data_client_connect_disconnect() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, _rx) = create_test_data_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_auth(&listener).await;
        tokio::time::sleep(Duration::from_secs(3)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();
    assert!(client.is_connected());
    assert!(state.login_count.load(std::sync::atomic::Ordering::Relaxed) > 0);

    client.disconnect().await.unwrap();
    assert!(client.is_disconnected());

    let _ = server.await;
}

#[rstest]
#[tokio::test]
async fn test_data_client_emits_instruments_on_connect() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx) = create_test_data_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_auth(&listener).await;
        tokio::time::sleep(Duration::from_secs(3)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    let mut instrument_count = 0;

    while let Ok(event) = rx.try_recv() {
        if matches!(event, DataEvent::Instrument(_)) {
            instrument_count += 1;
        }
    }

    assert!(
        instrument_count > 0,
        "Expected at least one Instrument event after connect, found 0"
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

#[rstest]
#[tokio::test]
async fn test_data_client_subscribe_sends_market_subscription() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, _rx) = create_test_data_client(addr, stream_port);

    let sub_received = Arc::new(tokio::sync::Mutex::new(String::new()));
    let sub_received2 = Arc::clone(&sub_received);

    let server = tokio::spawn(async move {
        let (mut reader, write_half) = accept_and_auth(&listener).await;

        // Capture the marketSubscription sent after the initial auth handshake
        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();

        *sub_received2.lock().await = line.trim().to_string();

        tokio::time::sleep(Duration::from_secs(2)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    let instrument_id = nautilus_betfair::common::parse::make_instrument_id(
        "1.180294978",
        6146434,
        rust_decimal::Decimal::ZERO,
    );

    let cmd = SubscribeBookDeltas::new(
        instrument_id,
        BookType::L2_MBP,
        None,
        Some(*BETFAIR_VENUE),
        UUID4::new(),
        nautilus_core::UnixNanos::default(),
        None,
        false,
        None,
        None,
    );
    client.subscribe_book_deltas(cmd).unwrap();

    wait_until_async(
        || {
            let s = Arc::clone(&sub_received);
            async move { !s.lock().await.is_empty() }
        },
        Duration::from_secs(5),
    )
    .await;

    let msg = sub_received.lock().await;
    let json: Value = serde_json::from_str(&msg).unwrap();
    assert_eq!(json["op"], "marketSubscription");

    client.disconnect().await.unwrap();
    let _ = server.await;
}

#[rstest]
#[tokio::test]
async fn test_data_client_deduplicates_same_market_subscription() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, _rx) = create_test_data_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (mut reader, write_half) = accept_and_auth(&listener).await;

        let mut first_line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut first_line)
            .await
            .unwrap();

        let mut second_line = String::new();
        let second_result = tokio::time::timeout(
            Duration::from_millis(500),
            tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut second_line),
        )
        .await;

        tokio::time::sleep(Duration::from_secs(1)).await;
        drop(write_half);

        (
            first_line.trim().to_string(),
            matches!(second_result, Ok(Ok(bytes)) if bytes > 0),
        )
    });

    client.connect().await.unwrap();

    let first_instrument_id = nautilus_betfair::common::parse::make_instrument_id(
        "1.180294978",
        6146434,
        rust_decimal::Decimal::ZERO,
    );
    let second_instrument_id = nautilus_betfair::common::parse::make_instrument_id(
        "1.180294978",
        40273293,
        rust_decimal::Decimal::ZERO,
    );

    let first_cmd = SubscribeBookDeltas::new(
        first_instrument_id,
        BookType::L2_MBP,
        None,
        Some(*BETFAIR_VENUE),
        UUID4::new(),
        nautilus_core::UnixNanos::default(),
        None,
        false,
        None,
        None,
    );
    let second_cmd = SubscribeBookDeltas::new(
        second_instrument_id,
        BookType::L2_MBP,
        None,
        Some(*BETFAIR_VENUE),
        UUID4::new(),
        nautilus_core::UnixNanos::default(),
        None,
        false,
        None,
        None,
    );

    client.subscribe_book_deltas(first_cmd).unwrap();
    client.subscribe_book_deltas(second_cmd).unwrap();

    let (first_msg, saw_second_message) = server.await.unwrap();
    let json: Value = serde_json::from_str(&first_msg).unwrap();
    assert_eq!(json["op"], "marketSubscription");
    assert!(
        !saw_second_message,
        "Expected only one subscription for the same market"
    );

    client.disconnect().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_mcm_handler_emits_book_deltas() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx) = create_test_data_client(addr, stream_port);

    let mcm_fixture = load_fixture("stream/mcm_UPDATE.json");

    let server = tokio::spawn(async move {
        let (_reader, mut write_half) = accept_and_auth(&listener).await;

        // Allow subscribe to complete before sending data
        tokio::time::sleep(Duration::from_millis(200)).await;

        tokio::io::AsyncWriteExt::write_all(
            &mut write_half,
            format!("{}\r\n", mcm_fixture.trim()).as_bytes(),
        )
        .await
        .unwrap();

        tokio::time::sleep(Duration::from_secs(2)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for MCM event")
        .expect("channel closed");

    assert!(
        matches!(event, DataEvent::Data(Data::Deltas(_))),
        "Expected Deltas event from MCM, found: {event:?}"
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

#[rstest]
#[tokio::test]
async fn test_mcm_handler_emits_trades() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx) = create_test_data_client(addr, stream_port);

    let mcm_fixture = load_fixture("stream/mcm_UPDATE_tv.json");

    let server = tokio::spawn(async move {
        let (_reader, mut write_half) = accept_and_auth(&listener).await;

        tokio::time::sleep(Duration::from_millis(200)).await;

        tokio::io::AsyncWriteExt::write_all(
            &mut write_half,
            format!("{}\r\n", mcm_fixture.trim()).as_bytes(),
        )
        .await
        .unwrap();

        tokio::time::sleep(Duration::from_secs(2)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let mut found_trade = false;

    for _ in 0..10 {
        match tokio::time::timeout(Duration::from_secs(3), rx.recv()).await {
            Ok(Some(DataEvent::Data(Data::Trade(_)))) => {
                found_trade = true;
                break;
            }
            Ok(Some(_)) => {}
            _ => break,
        }
    }

    assert!(found_trade, "Expected Trade event from MCM with trd field");

    client.disconnect().await.unwrap();
    let _ = server.await;
}

#[rstest]
#[tokio::test]
async fn test_data_client_handles_heartbeat_gracefully() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx) = create_test_data_client(addr, stream_port);

    let heartbeat_fixture = load_fixture("stream/mcm_HEARTBEAT.json");

    let server = tokio::spawn(async move {
        let (_reader, mut write_half) = accept_and_auth(&listener).await;

        tokio::time::sleep(Duration::from_millis(200)).await;

        tokio::io::AsyncWriteExt::write_all(
            &mut write_half,
            format!("{}\r\n", heartbeat_fixture.trim()).as_bytes(),
        )
        .await
        .unwrap();

        tokio::time::sleep(Duration::from_secs(2)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    // Heartbeats should not produce data events
    let result = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await;
    assert!(
        result.is_err(),
        "Expected no data events after heartbeat, found: {result:?}"
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

#[rstest]
#[tokio::test]
async fn test_data_client_emits_instrument_before_status_on_market_definition() {
    // A single market-definition message must emit the `Instrument` event before
    // any `InstrumentStatus` so downstream consumers (e.g. the DataEngine)
    // cache the instrument before the status event references it. The status
    // action must match the fixture's market-level state (SUSPENDED → Pause,
    // with all-ACTIVE runners so the runner-level override does not fire).
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx) = create_test_data_client(addr, stream_port);

    let md_fixture = load_fixture("stream/mcm_UPDATE_md.json");

    let server = tokio::spawn(async move {
        let (_reader, mut write_half) = accept_and_auth(&listener).await;

        tokio::time::sleep(Duration::from_millis(200)).await;

        tokio::io::AsyncWriteExt::write_all(
            &mut write_half,
            format!("{}\r\n", md_fixture.trim()).as_bytes(),
        )
        .await
        .unwrap();

        tokio::time::sleep(Duration::from_secs(2)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let mut instrument_arrival: Option<usize> = None;
    let mut status_arrival: Option<usize> = None;
    let mut status_action: Option<MarketStatusAction> = None;
    let mut seen = 0;

    for _ in 0..30 {
        match tokio::time::timeout(Duration::from_secs(3), rx.recv()).await {
            Ok(Some(DataEvent::Instrument(_))) => {
                if instrument_arrival.is_none() {
                    instrument_arrival = Some(seen);
                }
                seen += 1;
            }
            Ok(Some(DataEvent::InstrumentStatus(event))) => {
                if status_arrival.is_none() {
                    status_arrival = Some(seen);
                    status_action = Some(event.action);
                }
                seen += 1;
            }
            Ok(Some(_)) => {
                seen += 1;
            }
            _ => break,
        }

        if instrument_arrival.is_some() && status_arrival.is_some() {
            break;
        }
    }

    let instr_idx = instrument_arrival.expect("expected an Instrument event");
    let status_idx = status_arrival.expect("expected an InstrumentStatus event");

    assert!(
        instr_idx < status_idx,
        "Instrument (arrival {instr_idx}) must precede InstrumentStatus (arrival {status_idx})"
    );
    assert_eq!(
        status_action,
        Some(MarketStatusAction::Pause),
        "SUSPENDED market must map to Pause status action"
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

#[rstest]
#[tokio::test]
async fn test_data_client_handles_sub_image_snapshot() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx) = create_test_data_client(addr, stream_port);

    let sub_image_fixture = load_fixture("stream/mcm_SUB_IMAGE.json");

    let server = tokio::spawn(async move {
        let (_reader, mut write_half) = accept_and_auth(&listener).await;

        tokio::time::sleep(Duration::from_millis(200)).await;

        tokio::io::AsyncWriteExt::write_all(
            &mut write_half,
            format!("{}\r\n", sub_image_fixture.trim()).as_bytes(),
        )
        .await
        .unwrap();

        tokio::time::sleep(Duration::from_secs(2)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let mut found_deltas = false;
    let mut found_instrument = false;

    for _ in 0..30 {
        match tokio::time::timeout(Duration::from_secs(3), rx.recv()).await {
            Ok(Some(DataEvent::Data(Data::Deltas(_)))) => {
                found_deltas = true;

                if found_instrument {
                    break;
                }
            }
            Ok(Some(DataEvent::Instrument(_))) => {
                found_instrument = true;

                if found_deltas {
                    break;
                }
            }
            Ok(Some(_)) => {}
            _ => break,
        }
    }

    assert!(
        found_deltas,
        "Expected Deltas events from SUB_IMAGE snapshot"
    );
    assert!(
        found_instrument,
        "Expected Instrument events from SUB_IMAGE market definition"
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

/// `RESUB_DELTA` MCMs arrive when the venue replays buffered changes after a
/// reconnect. They must reach the parser and surface as Deltas events for the
/// affected markets, not be silently dropped.
#[rstest]
#[tokio::test]
async fn test_data_client_handles_resub_delta_emits_deltas() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx) = create_test_data_client(addr, stream_port);

    let resub_fixture = load_fixture("stream/mcm_RESUB_DELTA.json");

    let server = tokio::spawn(async move {
        let (_reader, mut write_half) = accept_and_auth(&listener).await;

        tokio::time::sleep(Duration::from_millis(200)).await;

        tokio::io::AsyncWriteExt::write_all(
            &mut write_half,
            format!("{}\r\n", resub_fixture.trim()).as_bytes(),
        )
        .await
        .unwrap();

        tokio::time::sleep(Duration::from_secs(2)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    // The fixture replays 3 markets, each with 3 runners (9 runner-deltas total).
    // Tracking distinct market ids (rather than a raw delta count) catches
    // regressions that drop entire markets while still emitting a few deltas.
    let expected_markets: std::collections::HashSet<&str> =
        ["1.176621195", "1.167249195", "1.175776462"]
            .into_iter()
            .collect();
    let mut markets_seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut delta_count = 0usize;

    for _ in 0..40 {
        match tokio::time::timeout(Duration::from_secs(3), rx.recv()).await {
            Ok(Some(DataEvent::Data(Data::Deltas(d)))) => {
                delta_count += 1;
                let symbol = d.instrument_id.symbol.as_str();

                if let Some((market, _)) = symbol.split_once('-') {
                    markets_seen.insert(market.to_string());
                }

                if markets_seen.len() >= expected_markets.len() {
                    break;
                }
            }
            Ok(Some(_)) => {}
            _ => break,
        }
    }

    let seen_str: std::collections::HashSet<&str> =
        markets_seen.iter().map(String::as_str).collect();
    assert!(
        expected_markets.is_subset(&seen_str),
        "RESUB_DELTA replay must emit deltas for every market in the fixture, \
         expected {expected_markets:?}, was {markets_seen:?} (delta_count={delta_count})",
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

/// Live race-style MCMs reach the parser and emit one Deltas event per runner
/// in `rc`. Both the snapshot (`img: true`) and the delta update flavours must
/// behave the same way; the difference is only the snapshot flag, not the
/// dispatch path.
#[rstest]
#[case::image("stream/mcm_live_IMAGE.json", 2)]
#[case::update("stream/mcm_live_UPDATE.json", 1)]
#[tokio::test]
async fn test_data_client_handles_live_race_message_emits_deltas(
    #[case] fixture_path: &str,
    #[case] expected_min_deltas: usize,
) {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx) = create_test_data_client(addr, stream_port);

    let fixture = load_fixture(fixture_path);

    let server = tokio::spawn(async move {
        let (_reader, mut write_half) = accept_and_auth(&listener).await;

        tokio::time::sleep(Duration::from_millis(200)).await;

        tokio::io::AsyncWriteExt::write_all(
            &mut write_half,
            format!("{}\r\n", fixture.trim()).as_bytes(),
        )
        .await
        .unwrap();

        tokio::time::sleep(Duration::from_secs(2)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let mut delta_count = 0;

    for _ in 0..40 {
        match tokio::time::timeout(Duration::from_secs(3), rx.recv()).await {
            Ok(Some(DataEvent::Data(Data::Deltas(_)))) => {
                delta_count += 1;
                if delta_count >= expected_min_deltas {
                    break;
                }
            }
            Ok(Some(_)) => {}
            _ => break,
        }
    }

    assert!(
        delta_count >= expected_min_deltas,
        "Expected >= {expected_min_deltas} Deltas events from {fixture_path}, was {delta_count}",
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

/// A BSP-settled MCM (`marketDefinition.status = CLOSED`, `bspReconciled =
/// true`) marks the market as finalised. Each runner must emit an
/// InstrumentStatus with `Close` action so strategies can wind down promptly.
#[rstest]
#[tokio::test]
async fn test_data_client_handles_bsp_settled_emits_close_status() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx) = create_test_data_client(addr, stream_port);

    let settled_fixture = load_fixture("stream/mcm_BSP_settled.json");

    let server = tokio::spawn(async move {
        let (_reader, mut write_half) = accept_and_auth(&listener).await;

        tokio::time::sleep(Duration::from_millis(200)).await;

        tokio::io::AsyncWriteExt::write_all(
            &mut write_half,
            format!("{}\r\n", settled_fixture.trim()).as_bytes(),
        )
        .await
        .unwrap();

        tokio::time::sleep(Duration::from_secs(2)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let mut close_status_seen = false;

    for _ in 0..40 {
        match tokio::time::timeout(Duration::from_secs(3), rx.recv()).await {
            Ok(Some(DataEvent::InstrumentStatus(event)))
                if event.action == MarketStatusAction::Close =>
            {
                close_status_seen = true;
                break;
            }
            Ok(Some(_)) => {}
            _ => break,
        }
    }

    assert!(
        close_status_seen,
        "BSP-settled MCM (CLOSED) must emit InstrumentStatus(Close)"
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

/// A BSP SUB_IMAGE frame carries both a market definition and runner-change
/// snapshots, so a single message must emit Instrument events ahead of the
/// Deltas events derived from `rc`.
#[rstest]
#[tokio::test]
async fn test_data_client_handles_bsp_sub_image_emits_instrument_and_deltas() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx) = create_test_data_client(addr, stream_port);

    // mcm_BSP.json is a 2-frame array; only frame[0] carries the market def
    // and the snapshot data we need to assert on.
    let bsp_body = load_fixture("stream/mcm_BSP.json");
    let bsp_value: Value = serde_json::from_str(&bsp_body).unwrap();
    let first_frame = bsp_value
        .as_array()
        .and_then(|arr| arr.first())
        .expect("expected at least one BSP frame")
        .to_string();

    let server = tokio::spawn(async move {
        let (_reader, mut write_half) = accept_and_auth(&listener).await;

        tokio::time::sleep(Duration::from_millis(200)).await;

        tokio::io::AsyncWriteExt::write_all(
            &mut write_half,
            format!("{first_frame}\r\n").as_bytes(),
        )
        .await
        .unwrap();

        tokio::time::sleep(Duration::from_secs(2)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let mut instrument_seen = false;
    let mut deltas_seen = false;
    let mut instrument_idx: Option<usize> = None;
    let mut deltas_idx: Option<usize> = None;
    let mut idx = 0;

    for _ in 0..40 {
        match tokio::time::timeout(Duration::from_secs(3), rx.recv()).await {
            Ok(Some(DataEvent::Instrument(_))) => {
                if !instrument_seen {
                    instrument_seen = true;
                    instrument_idx = Some(idx);
                }
                idx += 1;
            }
            Ok(Some(DataEvent::Data(Data::Deltas(_)))) => {
                if !deltas_seen {
                    deltas_seen = true;
                    deltas_idx = Some(idx);
                }
                idx += 1;
            }
            Ok(Some(_)) => {
                idx += 1;
            }
            _ => break,
        }

        if instrument_seen && deltas_seen {
            break;
        }
    }

    assert!(instrument_seen, "BSP SUB_IMAGE must emit Instrument events");
    assert!(deltas_seen, "BSP SUB_IMAGE must emit Deltas events");

    let instr = instrument_idx.unwrap();
    let deltas = deltas_idx.unwrap();
    assert!(
        instr <= deltas,
        "Instrument (idx {instr}) must be emitted no later than Deltas (idx {deltas}) so downstream caches see the instrument first",
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

/// `disconnect()` on a never-connected client is a no-op rather than an
/// error so caller cleanup paths can be unconditional.
#[rstest]
#[tokio::test]
async fn test_data_client_disconnect_when_never_connected_is_noop() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, _listener) = start_mock_stream().await;
    let (mut client, _rx) = create_test_data_client(addr, stream_port);

    assert!(client.is_disconnected());
    client.disconnect().await.unwrap();
    assert!(client.is_disconnected());
}

/// A second `connect()` on a live data client must short-circuit: no extra
/// login, no second instrument-provider load, no second stream socket.
/// Without the data-client-level guard a strategy that re-issues `connect()`
/// (eg. via a reconciliation hook) would re-flood the bus with Instrument
/// events and open a second stream socket, even though the HTTP client is
/// itself idempotent and the login count would still read 1.
#[rstest]
#[tokio::test]
async fn test_data_client_connect_is_idempotent() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx) = create_test_data_client(addr, stream_port);

    // Track every accept on the mock stream socket. A non-idempotent data
    // client would open a second connection on the second connect() call.
    let stream_accepts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let stream_accepts_server = Arc::clone(&stream_accepts);

    let server = tokio::spawn(async move {
        loop {
            tokio::select! {
                accepted = listener.accept() => {
                    let Ok((socket, _)) = accepted else { break };
                    // Count immediately, before any handshake.
                    stream_accepts_server
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

                    // Hand off the socket to a per-connection task that drives
                    // the auth handshake and then drains until EOF, so the
                    // accept loop is free to observe a second connect().
                    tokio::spawn(async move {
                        let (read_half, mut write_half) = socket.into_split();
                        let mut reader = tokio::io::BufReader::new(read_half);

                        let _ = tokio::io::AsyncWriteExt::write_all(
                            &mut write_half,
                            b"{\"op\":\"connection\",\"connectionId\":\"idempotent\"}\r\n",
                        )
                        .await;

                        let mut auth = String::new();
                        let _ =
                            tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut auth).await;

                        loop {
                            let mut buf = String::new();
                            match tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut buf).await
                            {
                                Ok(0) | Err(_) => break,
                                Ok(_) => {}
                            }
                        }
                    });
                }
                () = tokio::time::sleep(Duration::from_secs(5)) => break,
            }
        }
    });

    client.connect().await.unwrap();
    let after_first_login = state.login_count.load(std::sync::atomic::Ordering::Relaxed);
    assert_eq!(after_first_login, 1, "first connect must login");

    // Drain instrument events emitted during connect-time provider load so
    // the second connect's behaviour is observable in isolation.
    let mut first_instruments = 0usize;

    while let Ok(event) = rx.try_recv() {
        if matches!(event, DataEvent::Instrument(_)) {
            first_instruments += 1;
        }
    }
    assert!(
        first_instruments > 0,
        "first connect must emit instrument events"
    );

    // The accept task records the count in a parallel future; poll until it
    // catches up rather than racing on a fixed delay.
    wait_until_async(
        || {
            let s = Arc::clone(&stream_accepts);
            async move { s.load(std::sync::atomic::Ordering::Relaxed) >= 1 }
        },
        Duration::from_secs(2),
    )
    .await;

    let stream_accepts_after_first = stream_accepts.load(std::sync::atomic::Ordering::Relaxed);
    assert_eq!(
        stream_accepts_after_first, 1,
        "first connect must open one stream socket"
    );

    client.connect().await.unwrap();

    // Allow any erroneous second-connect plumbing time to land.
    tokio::time::sleep(Duration::from_millis(200)).await;

    let after_second_login = state.login_count.load(std::sync::atomic::Ordering::Relaxed);
    assert_eq!(
        after_second_login, 1,
        "second connect on a live client must short-circuit (no extra login)",
    );

    let stream_accepts_after_second = stream_accepts.load(std::sync::atomic::Ordering::Relaxed);
    assert_eq!(
        stream_accepts_after_second, 1,
        "second connect must not open a second stream socket",
    );

    let mut second_instruments = 0usize;

    while let Ok(event) = rx.try_recv() {
        if matches!(event, DataEvent::Instrument(_)) {
            second_instruments += 1;
        }
    }
    assert_eq!(
        second_instruments, 0,
        "second connect must not re-flood the bus with instrument events"
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

#[rstest]
#[tokio::test]
async fn test_data_client_reset_clears_state() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, _rx) = create_test_data_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_auth(&listener).await;
        tokio::time::sleep(Duration::from_secs(3)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();
    assert!(client.is_connected());

    client.reset().unwrap();
    assert!(client.is_disconnected());

    let _ = server.await;
}

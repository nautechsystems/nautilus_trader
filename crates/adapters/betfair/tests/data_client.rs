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
    config::BetfairDataConfig, data::BetfairDataClient, provider::NavigationFilter,
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
    identifiers::{ClientId, Venue},
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
        ClientId::from("BETFAIR"),
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
        Some(Venue::from("BETFAIR")),
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
        Some(Venue::from("BETFAIR")),
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
        Some(Venue::from("BETFAIR")),
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

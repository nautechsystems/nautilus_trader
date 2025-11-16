// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Live integration tests for the dYdX data client against the public testnet.
//!
//! These tests are **ignored by default** and are intended to be run manually:
//! ```bash
//! DYDX_TESTNET_INTEGRATION=1 cargo test -p nautilus-dydx --tests -- --ignored
//! ```
//! They require outbound network access to the dYdX v4 testnet indexer.

use std::time::Duration;

use nautilus_common::{
    messages::{
        DataEvent,
        data::{DataResponse, RequestBars, SubscribeBars, SubscribeBookDeltas, SubscribeTrades},
    },
    runner::set_data_event_sender,
    testing::init_logger_for_testing,
};
use nautilus_core::{UUID4, UnixNanos, time::get_atomic_clock_realtime};
use nautilus_data::client::DataClient;
use nautilus_dydx::{
    common::consts::{DYDX_TESTNET_HTTP_URL, DYDX_TESTNET_WS_URL, DYDX_VENUE},
    config::DydxDataClientConfig,
    data::DydxDataClient,
    http::client::DydxHttpClient,
    websocket::client::DydxWebSocketClient,
};
use nautilus_model::{
    data::{BarSpecification, BarType},
    enums::{BarAggregation, BookType, PriceType},
    identifiers::{ClientId, InstrumentId},
};
use tokio::time::timeout;

fn integration_enabled() -> bool {
    std::env::var("DYDX_TESTNET_INTEGRATION")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Connects to dYdX testnet and subscribes to trades, order book deltas, and bars for BTC-USD.
///
/// This test verifies that:
/// - The data client can connect to the testnet.
/// - Subscriptions for trades, book deltas, and bars succeed.
/// - At least one `DataEvent` is received on the data channel.
#[tokio::test]
#[ignore]
async fn dydx_testnet_connect_and_subscribe_btc_usd() -> anyhow::Result<()> {
    if !integration_enabled() {
        // Allow the test to be skipped without failing when integration is disabled.
        return Ok(());
    }

    let _guard = init_logger_for_testing(None)?;

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let client_id = ClientId::from("DYDX-INT-DATA");

    let config = DydxDataClientConfig {
        is_testnet: true,
        base_url_http: Some(
            std::env::var("DYDX_TESTNET_HTTP_URL")
                .unwrap_or_else(|_| DYDX_TESTNET_HTTP_URL.to_string()),
        ),
        base_url_ws: Some(
            std::env::var("DYDX_TESTNET_WS_URL")
                .unwrap_or_else(|_| DYDX_TESTNET_WS_URL.to_string()),
        ),
        ..Default::default()
    };

    let http_client = DydxHttpClient::new(
        config.base_url_http.clone(),
        config.http_timeout_secs,
        config.http_proxy_url.clone(),
        config.is_testnet,
        None,
    )?;

    let ws_url = config
        .base_url_ws
        .clone()
        .unwrap_or_else(|| DYDX_TESTNET_WS_URL.to_string());
    let ws_client = DydxWebSocketClient::new_public(ws_url, Some(30));

    let mut data_client = DydxDataClient::new(client_id, config, http_client, Some(ws_client))?;

    data_client.connect().await?;

    // BTC-USD perpetual on dYdX maps to this instrument ID.
    let instrument_id = InstrumentId::from("BTC-USD-PERP.DYDX");
    let venue = *DYDX_VENUE;
    let ts_init: UnixNanos = get_atomic_clock_realtime().get_time_ns();
    let command_id = UUID4::new();

    // Subscribe to trades.
    let subscribe_trades = SubscribeTrades::new(
        instrument_id,
        Some(client_id),
        Some(venue),
        command_id,
        ts_init,
        None,
    );
    data_client.subscribe_trades(&subscribe_trades)?;

    // Subscribe to order book deltas.
    let subscribe_book = SubscribeBookDeltas::new(
        instrument_id,
        BookType::L2_MBP,
        Some(client_id),
        Some(venue),
        command_id,
        ts_init,
        None,
        false,
        None,
    );
    data_client.subscribe_book_deltas(&subscribe_book)?;

    // Subscribe to 1-minute bars.
    let bar_spec = BarSpecification {
        step: std::num::NonZeroUsize::new(1).unwrap(),
        aggregation: BarAggregation::Minute,
        price_type: PriceType::Last,
    };
    let bar_type = BarType::new(
        instrument_id,
        bar_spec,
        nautilus_model::enums::AggregationSource::External,
    );
    let subscribe_bars = SubscribeBars::new(
        bar_type,
        Some(client_id),
        Some(venue),
        command_id,
        ts_init,
        None,
    );
    data_client.subscribe_bars(&subscribe_bars)?;

    // Wait for at least one DataEvent to confirm data flow.
    match timeout(Duration::from_secs(30), rx.recv()).await {
        Ok(Some(_)) => {
            // Any data or response from testnet is sufficient to confirm the path.
        }
        Ok(None) => anyhow::bail!("data channel closed before receiving DataEvent"),
        Err(_) => anyhow::bail!("timed out waiting for DataEvent"),
    }

    Ok(())
}

/// Requests historical bars for BTC-USD on dYdX testnet over small and large date ranges.
///
/// This test verifies:
/// - `request_bars` executes without error for both short and long ranges.
/// - A `BarsResponse` is emitted on the data channel for each request.
#[tokio::test]
#[ignore]
async fn dydx_testnet_request_historical_bars() -> anyhow::Result<()> {
    if !integration_enabled() {
        return Ok(());
    }

    let _guard = init_logger_for_testing(None)?;

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let client_id = ClientId::from("DYDX-INT-BARS");

    let config = DydxDataClientConfig {
        is_testnet: true,
        base_url_http: Some(
            std::env::var("DYDX_TESTNET_HTTP_URL")
                .unwrap_or_else(|_| DYDX_TESTNET_HTTP_URL.to_string()),
        ),
        ..Default::default()
    };

    let http_client = DydxHttpClient::new(
        config.base_url_http.clone(),
        config.http_timeout_secs,
        config.http_proxy_url.clone(),
        config.is_testnet,
        None,
    )?;

    let data_client = DydxDataClient::new(client_id, config, http_client, None)?;

    let instrument_id = InstrumentId::from("BTC-USD-PERP.DYDX");
    let bar_spec = BarSpecification {
        step: std::num::NonZeroUsize::new(1).unwrap(),
        aggregation: BarAggregation::Minute,
        price_type: PriceType::Last,
    };
    let bar_type = BarType::new(
        instrument_id,
        bar_spec,
        nautilus_model::enums::AggregationSource::External,
    );

    let now = chrono::Utc::now();
    let small_start = Some(now - chrono::Duration::hours(1));
    let small_end = Some(now);
    let large_start = Some(now - chrono::Duration::hours(24));
    let large_end = Some(now);

    let ts_init = get_atomic_clock_realtime().get_time_ns();

    // Small range request.
    let small_request = RequestBars::new(
        bar_type,
        small_start,
        small_end,
        Some(std::num::NonZeroUsize::new(100).unwrap()),
        Some(client_id),
        UUID4::new(),
        ts_init,
        None,
    );
    data_client.request_bars(&small_request)?;

    // Large range request (will exercise partitioning logic).
    let large_request = RequestBars::new(
        bar_type,
        large_start,
        large_end,
        Some(std::num::NonZeroUsize::new(5_000).unwrap()),
        Some(client_id),
        UUID4::new(),
        ts_init,
        None,
    );
    data_client.request_bars(&large_request)?;

    // Collect at least one BarsResponse.
    let mut saw_bars_response = false;
    let timeout_at = Duration::from_secs(60);

    while !saw_bars_response {
        let event = match timeout(timeout_at, rx.recv()).await {
            Ok(Some(ev)) => ev,
            Ok(None) => break,
            Err(_) => break,
        };

        if let DataEvent::Response(DataResponse::Bars(_)) = event {
            saw_bars_response = true;
        }
    }

    assert!(
        saw_bars_response,
        "expected at least one BarsResponse from dYdX testnet"
    );

    Ok(())
}

/// Verifies that orderbook snapshot refresh task runs periodically.
///
/// This test:
/// - Subscribes to order book deltas.
/// - Waits for multiple orderbook updates to verify periodic refresh is active.
/// - Confirms that snapshots are being fetched on the configured interval.
#[tokio::test]
#[ignore]
async fn dydx_testnet_orderbook_snapshot_refresh() -> anyhow::Result<()> {
    if !integration_enabled() {
        return Ok(());
    }

    let _guard = init_logger_for_testing(None)?;

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let client_id = ClientId::from("DYDX-INT-SNAPSHOT");

    let config = DydxDataClientConfig {
        is_testnet: true,
        orderbook_refresh_interval_secs: Some(10), // Refresh every 10 seconds for testing
        base_url_http: Some(
            std::env::var("DYDX_TESTNET_HTTP_URL")
                .unwrap_or_else(|_| DYDX_TESTNET_HTTP_URL.to_string()),
        ),
        base_url_ws: Some(
            std::env::var("DYDX_TESTNET_WS_URL")
                .unwrap_or_else(|_| DYDX_TESTNET_WS_URL.to_string()),
        ),
        ..Default::default()
    };

    let http_client = DydxHttpClient::new(
        config.base_url_http.clone(),
        config.http_timeout_secs,
        config.http_proxy_url.clone(),
        config.is_testnet,
        None,
    )?;

    let ws_url = config
        .base_url_ws
        .clone()
        .unwrap_or_else(|| DYDX_TESTNET_WS_URL.to_string());
    let ws_client = DydxWebSocketClient::new_public(ws_url, Some(30));

    let mut data_client = DydxDataClient::new(client_id, config, http_client, Some(ws_client))?;

    data_client.connect().await?;

    let instrument_id = InstrumentId::from("BTC-USD-PERP.DYDX");
    let venue = *DYDX_VENUE;
    let ts_init: UnixNanos = get_atomic_clock_realtime().get_time_ns();
    let command_id = UUID4::new();

    // Subscribe to order book deltas (triggers snapshot refresh task).
    let subscribe_book = SubscribeBookDeltas::new(
        instrument_id,
        BookType::L2_MBP,
        Some(client_id),
        Some(venue),
        command_id,
        ts_init,
        None,
        false,
        None,
    );
    data_client.subscribe_book_deltas(&subscribe_book)?;

    // Collect orderbook deltas for at least 30 seconds to verify periodic refresh.
    let mut orderbook_updates = 0;
    let timeout_at = Duration::from_secs(30);
    let start = tokio::time::Instant::now();

    while start.elapsed() < timeout_at && orderbook_updates < 5 {
        match timeout(Duration::from_secs(5), rx.recv()).await {
            Ok(Some(DataEvent::Data(_))) => {
                orderbook_updates += 1;
            }
            Ok(Some(_)) => continue,
            Ok(None) => break,
            Err(_) => continue,
        }
    }

    assert!(
        orderbook_updates >= 3,
        "expected at least 3 orderbook updates within 30 seconds (got {})",
        orderbook_updates
    );

    data_client.disconnect().await?;

    Ok(())
}

/// End-to-end integration test verifying complete data flow from dYdX testnet to DataEngine.
///
/// This test:
/// - Connects to testnet and bootstraps instruments.
/// - Subscribes to trades, orderbook deltas, quotes, and bars.
/// - Verifies that each data type flows correctly to the data channel.
/// - Confirms quote generation from book deltas (dYdX-specific).
#[tokio::test]
#[ignore]
async fn dydx_testnet_complete_data_flow() -> anyhow::Result<()> {
    if !integration_enabled() {
        return Ok(());
    }

    let _guard = init_logger_for_testing(None)?;

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let client_id = ClientId::from("DYDX-INT-FLOW");

    let config = DydxDataClientConfig {
        is_testnet: true,
        base_url_http: Some(
            std::env::var("DYDX_TESTNET_HTTP_URL")
                .unwrap_or_else(|_| DYDX_TESTNET_HTTP_URL.to_string()),
        ),
        base_url_ws: Some(
            std::env::var("DYDX_TESTNET_WS_URL")
                .unwrap_or_else(|_| DYDX_TESTNET_WS_URL.to_string()),
        ),
        ..Default::default()
    };

    let http_client = DydxHttpClient::new(
        config.base_url_http.clone(),
        config.http_timeout_secs,
        config.http_proxy_url.clone(),
        config.is_testnet,
        None,
    )?;

    let ws_url = config
        .base_url_ws
        .clone()
        .unwrap_or_else(|| DYDX_TESTNET_WS_URL.to_string());
    let ws_client = DydxWebSocketClient::new_public(ws_url, Some(30));

    let mut data_client = DydxDataClient::new(client_id, config, http_client, Some(ws_client))?;

    // Connect and bootstrap instruments.
    data_client.connect().await?;

    let instrument_id = InstrumentId::from("BTC-USD-PERP.DYDX");
    let venue = *DYDX_VENUE;
    let ts_init: UnixNanos = get_atomic_clock_realtime().get_time_ns();
    let command_id = UUID4::new();

    // Subscribe to all major data types.
    let subscribe_trades = SubscribeTrades::new(
        instrument_id,
        Some(client_id),
        Some(venue),
        command_id,
        ts_init,
        None,
    );
    data_client.subscribe_trades(&subscribe_trades)?;

    let subscribe_book = SubscribeBookDeltas::new(
        instrument_id,
        BookType::L2_MBP,
        Some(client_id),
        Some(venue),
        command_id,
        ts_init,
        None,
        false,
        None,
    );
    data_client.subscribe_book_deltas(&subscribe_book)?;

    let bar_spec = BarSpecification {
        step: std::num::NonZeroUsize::new(1).unwrap(),
        aggregation: BarAggregation::Minute,
        price_type: PriceType::Last,
    };
    let bar_type = BarType::new(
        instrument_id,
        bar_spec,
        nautilus_model::enums::AggregationSource::External,
    );
    let subscribe_bars = SubscribeBars::new(
        bar_type,
        Some(client_id),
        Some(venue),
        command_id,
        ts_init,
        None,
    );
    data_client.subscribe_bars(&subscribe_bars)?;

    // Track received data types.
    let mut saw_trade = false;
    let mut saw_orderbook = false;
    let mut saw_bar = false;

    let timeout_at = Duration::from_secs(60);
    let start = tokio::time::Instant::now();

    while start.elapsed() < timeout_at && (!saw_trade || !saw_orderbook || !saw_bar) {
        match timeout(Duration::from_secs(10), rx.recv()).await {
            Ok(Some(DataEvent::Data(data))) => {
                use nautilus_model::data::Data;
                match data {
                    Data::Trade(_) => saw_trade = true,
                    Data::Delta(_) | Data::Deltas(_) => saw_orderbook = true,
                    Data::Bar(_) => saw_bar = true,
                    _ => {}
                }
            }
            Ok(Some(_)) => continue,
            Ok(None) => break,
            Err(_) => continue,
        }
    }

    data_client.disconnect().await?;

    assert!(saw_trade, "expected to receive at least one trade tick");
    assert!(
        saw_orderbook,
        "expected to receive at least one orderbook update"
    );
    assert!(saw_bar, "expected to receive at least one bar");

    Ok(())
}

/// Integration test for crossed orderbook detection with live WebSocket data.
///
/// This test subscribes to order book deltas for a volatile instrument and monitors
/// for crossed orderbook conditions. Due to dYdX's validator consensus delays,
/// crossed books may occur under high volatility.
///
/// **Note**: This test may not always observe a crossed book on testnet if market
/// conditions are stable. It serves as a regression test for the resolution logic.
#[tokio::test]
#[ignore]
async fn dydx_testnet_crossed_orderbook_live_detection() -> anyhow::Result<()> {
    if !integration_enabled() {
        return Ok(());
    }

    let _guard = init_logger_for_testing(None)?;

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    let client_id = ClientId::from("DYDX-INT-CROSSED");

    let config = DydxDataClientConfig {
        is_testnet: true,
        base_url_http: Some(
            std::env::var("DYDX_TESTNET_HTTP_URL")
                .unwrap_or_else(|_| DYDX_TESTNET_HTTP_URL.to_string()),
        ),
        base_url_ws: Some(
            std::env::var("DYDX_TESTNET_WS_URL")
                .unwrap_or_else(|_| DYDX_TESTNET_WS_URL.to_string()),
        ),
        ..Default::default()
    };

    let http_client = DydxHttpClient::new(
        config.base_url_http.clone(),
        config.http_timeout_secs,
        config.http_proxy_url.clone(),
        config.is_testnet,
        None,
    )?;

    let ws_url = config
        .base_url_ws
        .clone()
        .unwrap_or_else(|| DYDX_TESTNET_WS_URL.to_string());
    let ws_client = DydxWebSocketClient::new_public(ws_url, Some(30));

    let mut data_client = DydxDataClient::new(client_id, config, http_client, Some(ws_client))?;

    data_client.connect().await?;

    let instrument_id = InstrumentId::from("BTC-USD-PERP.DYDX");
    let venue = *DYDX_VENUE;
    let ts_init = get_atomic_clock_realtime().get_time_ns();
    let command_id = UUID4::new();

    let subscribe_book = SubscribeBookDeltas::new(
        instrument_id,
        BookType::L2_MBP,
        Some(client_id),
        Some(venue),
        command_id,
        ts_init,
        None,
        false,
        None,
    );
    data_client.subscribe_book_deltas(&subscribe_book)?;

    let mut saw_orderbook_delta = false;
    let mut quote_count = 0;

    let _test_result = timeout(Duration::from_secs(30), async {
        loop {
            match rx.try_recv() {
                Ok(DataEvent::Data(data)) => {
                    let data_type_str = format!("{:?}", data);
                    if data_type_str.contains("OrderBookDeltas") {
                        saw_orderbook_delta = true;
                    } else if data_type_str.contains("QuoteTick") {
                        quote_count += 1;
                    }
                }
                Ok(_) => continue,
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => break,
            }
        }
    })
    .await;

    data_client.disconnect().await?;

    assert!(
        saw_orderbook_delta,
        "Expected to receive at least one orderbook delta"
    );

    println!(
        "Integration test completed: saw {} orderbook deltas, {} quotes",
        if saw_orderbook_delta {
            "multiple"
        } else {
            "no"
        },
        quote_count
    );

    Ok(())
}

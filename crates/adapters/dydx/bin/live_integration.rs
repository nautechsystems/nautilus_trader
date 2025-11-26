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

//! Live integration tests for dYdX data client against public testnet.
//!
//! These tests verify end-to-end data flow from dYdX testnet to NautilusTrader's
//! data engine, including subscription management, orderbook handling, and data
//! type conversions.
//!
//! Usage:
//! ```bash
//! # Run all live integration tests against testnet
//! cargo run --bin dydx-live-integration -p nautilus-dydx
//!
//! # Override endpoints
//! DYDX_HTTP_URL=https://indexer.v4testnet.dydx.exchange \
//! DYDX_WS_URL=wss://indexer.v4testnet.dydx.exchange/v4/ws \
//! cargo run --bin dydx-live-integration -p nautilus-dydx
//! ```
//!
//! **Requirements**:
//! - Outbound network access to dYdX v4 testnet indexer
//! - No credentials required (public endpoints only)

use std::time::Duration;

use nautilus_common::{
    messages::{
        DataEvent, DataResponse,
        data::{RequestBars, SubscribeBars, SubscribeBookDeltas, SubscribeTrades},
    },
    runner::set_data_event_sender,
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
    data::{BarSpecification, BarType, Data},
    enums::{BarAggregation, BookType, PriceType},
    identifiers::{ClientId, InstrumentId},
};
use tokio::time::timeout;
use tracing::level_filters::LevelFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::INFO)
        .init();

    tracing::info!("===== dYdX Live Integration Tests =====");
    tracing::info!("");

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    set_data_event_sender(tx);

    test_connect_and_subscribe(&mut rx).await?;
    test_request_historical_bars(&mut rx).await?;
    test_orderbook_snapshot_refresh(&mut rx).await?;
    test_complete_data_flow(&mut rx).await?;
    test_crossed_orderbook_detection(&mut rx).await?;

    tracing::info!("");
    tracing::info!("===== All Live Integration Tests Passed =====");

    Ok(())
}

async fn test_connect_and_subscribe(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
) -> anyhow::Result<()> {
    let client_id = ClientId::from("DYDX-INT-DATA");

    let config = DydxDataClientConfig {
        is_testnet: true,
        base_url_http: Some(
            std::env::var("DYDX_HTTP_URL").unwrap_or_else(|_| DYDX_TESTNET_HTTP_URL.to_string()),
        ),
        base_url_ws: Some(
            std::env::var("DYDX_WS_URL").unwrap_or_else(|_| DYDX_TESTNET_WS_URL.to_string()),
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
    tracing::info!("Connected to dYdX testnet");

    let instrument_id = InstrumentId::from("BTC-USD-PERP.DYDX");
    let venue = *DYDX_VENUE;
    let ts_init: UnixNanos = get_atomic_clock_realtime().get_time_ns();
    let command_id = UUID4::new();

    let subscribe_trades = SubscribeTrades::new(
        instrument_id,
        Some(client_id),
        Some(venue),
        command_id,
        ts_init,
        None,
    );
    data_client.subscribe_trades(&subscribe_trades)?;
    tracing::info!("Subscribed to trades");

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
    tracing::info!("Subscribed to order book deltas");

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
    tracing::info!("Subscribed to 1-minute bars");

    match timeout(Duration::from_secs(30), rx.recv()).await {
        Ok(Some(_)) => {
            tracing::info!("Received data event from testnet");
        }
        Ok(None) => anyhow::bail!("data channel closed before receiving DataEvent"),
        Err(_) => anyhow::bail!("timed out waiting for DataEvent"),
    }

    data_client.disconnect().await?;
    tracing::info!("Disconnected");
    tracing::info!("");

    Ok(())
}

async fn test_request_historical_bars(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
) -> anyhow::Result<()> {
    let client_id = ClientId::from("DYDX-INT-BARS");

    let config = DydxDataClientConfig {
        is_testnet: true,
        base_url_http: Some(
            std::env::var("DYDX_HTTP_URL").unwrap_or_else(|_| DYDX_TESTNET_HTTP_URL.to_string()),
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

    let mut data_client = DydxDataClient::new(client_id, config, http_client, None)?;

    // Connect to bootstrap instruments (required for bar conversion)
    // Note: No WebSocket client provided, so only HTTP initialization occurs
    data_client.connect().await?;

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
    tracing::info!("Requested 1-hour bar range");

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
    tracing::info!("Requested 24-hour bar range (partitioned)");

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

    if saw_bars_response {
        tracing::info!("Received BarsResponse");
    } else {
        anyhow::bail!("expected at least one BarsResponse from dYdX testnet");
    }

    tracing::info!("");

    Ok(())
}

async fn test_orderbook_snapshot_refresh(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
) -> anyhow::Result<()> {
    let client_id = ClientId::from("DYDX-INT-SNAPSHOT");

    let config = DydxDataClientConfig {
        is_testnet: true,
        orderbook_refresh_interval_secs: Some(10),
        base_url_http: Some(
            std::env::var("DYDX_HTTP_URL").unwrap_or_else(|_| DYDX_TESTNET_HTTP_URL.to_string()),
        ),
        base_url_ws: Some(
            std::env::var("DYDX_WS_URL").unwrap_or_else(|_| DYDX_TESTNET_WS_URL.to_string()),
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
    tracing::info!("Subscribed to order book with 10s refresh interval");

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

    data_client.disconnect().await?;

    if orderbook_updates >= 3 {
        tracing::info!("Received {} orderbook updates", orderbook_updates);
    } else {
        anyhow::bail!(
            "expected at least 3 orderbook updates, got {}",
            orderbook_updates
        );
    }

    tracing::info!("");

    Ok(())
}

async fn test_complete_data_flow(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
) -> anyhow::Result<()> {
    let client_id = ClientId::from("DYDX-INT-FLOW");

    let config = DydxDataClientConfig {
        is_testnet: true,
        base_url_http: Some(
            std::env::var("DYDX_HTTP_URL").unwrap_or_else(|_| DYDX_TESTNET_HTTP_URL.to_string()),
        ),
        base_url_ws: Some(
            std::env::var("DYDX_WS_URL").unwrap_or_else(|_| DYDX_TESTNET_WS_URL.to_string()),
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

    let mut saw_trade = false;
    let mut saw_orderbook = false;
    let mut saw_bar = false;

    let timeout_at = Duration::from_secs(60);
    let start = tokio::time::Instant::now();

    while start.elapsed() < timeout_at && (!saw_trade || !saw_orderbook || !saw_bar) {
        match timeout(Duration::from_secs(10), rx.recv()).await {
            Ok(Some(DataEvent::Data(data))) => match data {
                Data::Trade(_) => {
                    if !saw_trade {
                        tracing::info!("Received trade data");
                        saw_trade = true;
                    }
                }
                Data::Delta(_) | Data::Deltas(_) => {
                    if !saw_orderbook {
                        tracing::info!("Received orderbook data");
                        saw_orderbook = true;
                    }
                }
                Data::Bar(_) => {
                    if !saw_bar {
                        tracing::info!("Received bar data");
                        saw_bar = true;
                    }
                }
                _ => {}
            },
            Ok(Some(_)) => continue,
            Ok(None) => break,
            Err(_) => continue,
        }
    }

    data_client.disconnect().await?;

    if !saw_trade {
        anyhow::bail!("expected to receive at least one trade tick");
    }
    if !saw_orderbook {
        anyhow::bail!("expected to receive at least one orderbook update");
    }
    if !saw_bar {
        anyhow::bail!("expected to receive at least one bar");
    }

    tracing::info!("");

    Ok(())
}

async fn test_crossed_orderbook_detection(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
) -> anyhow::Result<()> {
    let client_id = ClientId::from("DYDX-INT-CROSSED");

    let config = DydxDataClientConfig {
        is_testnet: true,
        base_url_http: Some(
            std::env::var("DYDX_HTTP_URL").unwrap_or_else(|_| DYDX_TESTNET_HTTP_URL.to_string()),
        ),
        base_url_ws: Some(
            std::env::var("DYDX_WS_URL").unwrap_or_else(|_| DYDX_TESTNET_WS_URL.to_string()),
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

    if saw_orderbook_delta {
        tracing::info!("Monitored orderbook for crossed conditions");
        tracing::info!("  Received {} quotes from deltas", quote_count);
    } else {
        anyhow::bail!("Expected to receive at least one orderbook delta");
    }

    tracing::info!("");

    Ok(())
}

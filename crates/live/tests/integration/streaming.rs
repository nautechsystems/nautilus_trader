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

//! End-to-end verification for Rust `LiveNode` streaming to Feather.
//!
//! Drives a simulated-live node (`start`/`stop`/`dispose`) on a current-thread Tokio runtime,
//! publishes market data through typed message-bus routes plus order events through the
//! order-event route, then proves the Feather output converts and round-trips through typed
//! catalog queries. The first test isolates size rotation (auto-flush disabled by an
//! unreachable interval) and the second isolates auto-flush (no rotation), so each
//! boundary is proven to fire synchronously from inside the publish callbacks.

use std::{str::FromStr, time::Duration};

use nautilus_common::{
    enums::Environment,
    msgbus::{self, switchboard},
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_live::{
    config::{LiveExecutionEngineConfig, LiveNodeConfig},
    node::{LiveNode, NodeState},
};
use nautilus_model::{
    data::{
        Bar, BarType, BookOrder, NautilusDataType, NautilusRecordType, OrderBookDelta,
        OrderBookDeltas, QuoteTick, TradeTick,
    },
    enums::{AggressorSide, BookAction, OrderSide, OrderType},
    identifiers::{AccountId, InstrumentId, StrategyId, TradeId, VenueOrderId},
    orders::{OrderTestBuilder, stubs::TestOrderEventStubs},
    types::{Price, Quantity},
};
use nautilus_persistence::{
    backend::parquet::catalog::ParquetDataCatalog, catalog::types::CatalogDataType,
};
use nautilus_system::config::{RotationConfig, StreamingConfig};
use rstest::rstest;

fn quote(instrument_id: InstrumentId, bid: &str, ask: &str, ts: u64) -> QuoteTick {
    QuoteTick::new(
        instrument_id,
        Price::from(bid),
        Price::from(ask),
        Quantity::from("1000"),
        Quantity::from("1000"),
        UnixNanos::from(ts),
        UnixNanos::from(ts),
    )
}

fn trade(instrument_id: InstrumentId, price: &str, ts: u64) -> TradeTick {
    TradeTick::new(
        instrument_id,
        Price::from(price),
        Quantity::from("10"),
        AggressorSide::Buy,
        TradeId::from(format!("T-{ts}")),
        UnixNanos::from(ts),
        UnixNanos::from(ts),
    )
}

/// Temporary catalog directory that is removed on drop, including on test failure.
struct CatalogTempDir(std::path::PathBuf);

impl CatalogTempDir {
    fn new(label: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("nautilus-live-streaming-{label}-{}", UUID4::new()));
        Self(path)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for CatalogTempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn feather_files_under(root: &std::path::Path, family: &str) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "feather")
                && path.to_string_lossy().contains(family)
            {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

#[rstest]
#[tokio::test(flavor = "current_thread")]
async fn test_livenode_streaming_records_typed_routes_to_feather() {
    let catalog_dir = CatalogTempDir::new("typed-routes");
    let instance_id = UUID4::new();
    let run_dir = catalog_dir
        .path()
        .join("live")
        .join(instance_id.to_string());

    // Unreachable auto-flush interval: only size rotation can produce feather files before stop
    let streaming = StreamingConfig::new(
        catalog_dir.path().to_string_lossy().into_owned(),
        "file".to_string(),
        3_600_000,
        false,
        RotationConfig::Size { max_size: 1 },
    );
    let config = LiveNodeConfig {
        environment: Environment::Live,
        instance_id: Some(instance_id),
        exec_engine: LiveExecutionEngineConfig {
            reconciliation: false,
            ..Default::default()
        },
        delay_post_stop: Duration::ZERO,
        timeout_connection: Duration::ZERO,
        timeout_disconnection: Duration::ZERO,
        streaming: Some(streaming),
        ..Default::default()
    };

    let mut node = LiveNode::build("LiveStreamingNode".to_string(), Some(config)).unwrap();
    node.start().await.unwrap();
    assert_eq!(node.handle().state(), NodeState::Running);

    let instrument_id = InstrumentId::from("ETHUSDT.BINANCE");
    let quotes = vec![
        quote(instrument_id, "3000.00", "3000.10", 1_000),
        quote(instrument_id, "3000.05", "3000.15", 2_000),
        quote(instrument_id, "3000.10", "3000.20", 3_000),
    ];
    let trades = vec![
        trade(instrument_id, "3000.12", 4_000),
        trade(instrument_id, "3000.18", 5_000),
    ];
    let delta = OrderBookDelta::new(
        instrument_id,
        BookAction::Add,
        BookOrder::new(
            OrderSide::Buy,
            Price::from("3000.00"),
            Quantity::from("5"),
            1,
        ),
        0,
        1,
        UnixNanos::from(6_000),
        UnixNanos::from(6_000),
    );
    let deltas = OrderBookDeltas::new(instrument_id, vec![delta]);
    let bar_type = BarType::from_str("ETHUSDT.BINANCE-1-MINUTE-LAST-EXTERNAL").unwrap();
    let bar = Bar::new_checked(
        bar_type,
        Price::from("3000.00"),
        Price::from("3001.00"),
        Price::from("2999.00"),
        Price::from("3000.50"),
        Quantity::from("100"),
        UnixNanos::from(7_000),
        UnixNanos::from(7_000),
    )
    .unwrap();

    // Typed routes only: the typed publish fns deliver exclusively to typed handlers, so every
    // captured row proves typed-route delivery with no drops or duplicates.
    let quotes_topic = switchboard::get_quotes_topic(instrument_id);
    for tick in &quotes {
        msgbus::publish_quote(quotes_topic, tick);
        std::thread::sleep(Duration::from_millis(2));
    }
    let trades_topic = switchboard::get_trades_topic(instrument_id);
    for tick in &trades {
        msgbus::publish_trade(trades_topic, tick);
        std::thread::sleep(Duration::from_millis(2));
    }
    msgbus::publish_deltas(switchboard::get_book_deltas_topic(instrument_id), &deltas);
    std::thread::sleep(Duration::from_millis(2));
    msgbus::publish_bar(switchboard::get_bars_topic(bar_type), &bar);

    let strategy_id = StrategyId::from("STREAM-001");
    let order = OrderTestBuilder::new(OrderType::Limit)
        .strategy_id(strategy_id)
        .instrument_id(instrument_id)
        .quantity(Quantity::from("1.0"))
        .price(Price::from("3000.00"))
        .build();
    let account_id = AccountId::from("BINANCE-001");
    let submitted = TestOrderEventStubs::submitted(&order, account_id);
    let accepted = TestOrderEventStubs::accepted(&order, account_id, VenueOrderId::from("V-001"));
    let order_topic = switchboard::get_event_order_topic(strategy_id);
    msgbus::publish_order_event(order_topic, &submitted);
    msgbus::publish_order_event(order_topic, &accepted);

    node.stop().await.unwrap();
    assert_eq!(node.handle().state(), NodeState::Stopped);
    node.dispose();

    // Every quote write rotates (`max_size: 1`), so at least one file per quote proves each
    // rotation fired synchronously inside its publish callback.
    let quote_files = feather_files_under(&run_dir, "quotes");
    assert!(
        quote_files.len() >= quotes.len(),
        "expected at least one feather file per quote write, found {}",
        quote_files.len(),
    );

    let mut catalog = ParquetDataCatalog::new(catalog_dir.path(), None, None, None, None);
    assert_eq!(
        catalog.list_live_runs().unwrap(),
        vec![instance_id.to_string()],
    );

    for data_type in [
        NautilusDataType::QuoteTick,
        NautilusDataType::TradeTick,
        NautilusDataType::OrderBookDelta,
        NautilusDataType::Bar,
    ] {
        catalog
            .convert_stream_to_data(
                &instance_id.to_string(),
                &CatalogDataType::from(data_type),
                Some("live"),
                None,
                false,
            )
            .unwrap();
    }
    assert_eq!(
        catalog
            .query_typed_data::<QuoteTick>(None, None, None, None, None, true)
            .unwrap(),
        quotes,
    );
    assert_eq!(
        catalog
            .query_typed_data::<TradeTick>(None, None, None, None, None, true)
            .unwrap(),
        trades,
    );
    assert_eq!(
        catalog
            .query_typed_data::<OrderBookDelta>(None, None, None, None, None, true)
            .unwrap(),
        vec![delta],
    );
    assert_eq!(
        catalog
            .query_typed_data::<Bar>(None, None, None, None, None, true)
            .unwrap(),
        vec![bar],
    );

    for record_type in [
        NautilusRecordType::OrderSubmitted,
        NautilusRecordType::OrderAccepted,
    ] {
        catalog
            .convert_stream_to_data(
                &instance_id.to_string(),
                &CatalogDataType::from(record_type),
                Some("live"),
                None,
                false,
            )
            .unwrap();
    }

    for record_type in [
        NautilusRecordType::OrderSubmitted,
        NautilusRecordType::OrderAccepted,
    ] {
        let batches = catalog
            .query_record_batches(
                &CatalogDataType::from(record_type),
                None,
                None,
                None,
                None,
                false,
            )
            .unwrap();
        let rows: usize = batches.iter().map(|batch| batch.num_rows()).sum();
        assert_eq!(rows, 1, "expected one {record_type} row");
    }
}

#[rstest]
#[tokio::test(flavor = "current_thread")]
async fn test_livenode_streaming_auto_flush_persists_before_stop() {
    // With `NoRotation`, any feather file present before `stop()` proves the
    // auto-flush boundary fired synchronously from inside a publish callback on
    // the LiveNode runtime thread.
    let catalog_dir = CatalogTempDir::new("auto-flush");
    let instance_id = UUID4::new();
    let run_dir = catalog_dir
        .path()
        .join("live")
        .join(instance_id.to_string());

    let streaming = StreamingConfig::new(
        catalog_dir.path().to_string_lossy().into_owned(),
        "file".to_string(),
        1,
        false,
        RotationConfig::NoRotation,
    );
    let config = LiveNodeConfig {
        environment: Environment::Live,
        instance_id: Some(instance_id),
        exec_engine: LiveExecutionEngineConfig {
            reconciliation: false,
            ..Default::default()
        },
        delay_post_stop: Duration::ZERO,
        timeout_connection: Duration::ZERO,
        timeout_disconnection: Duration::ZERO,
        streaming: Some(streaming),
        ..Default::default()
    };

    let mut node = LiveNode::build("LiveStreamingNode".to_string(), Some(config)).unwrap();
    node.start().await.unwrap();
    assert_eq!(node.handle().state(), NodeState::Running);

    let instrument_id = InstrumentId::from("ETHUSDT.BINANCE");
    let quotes = vec![
        quote(instrument_id, "3000.00", "3000.10", 1_000),
        quote(instrument_id, "3000.05", "3000.15", 2_000),
    ];
    let quotes_topic = switchboard::get_quotes_topic(instrument_id);
    for tick in &quotes {
        msgbus::publish_quote(quotes_topic, tick);
        std::thread::sleep(Duration::from_millis(2));
    }

    assert!(
        !feather_files_under(&run_dir, "quotes").is_empty(),
        "expected auto-flush to persist quotes before stop",
    );

    node.stop().await.unwrap();
    node.dispose();

    let mut catalog = ParquetDataCatalog::new(catalog_dir.path(), None, None, None, None);
    catalog
        .convert_stream_to_data(
            &instance_id.to_string(),
            &CatalogDataType::from(NautilusDataType::QuoteTick),
            Some("live"),
            None,
            false,
        )
        .unwrap();
    assert_eq!(
        catalog
            .query_typed_data::<QuoteTick>(None, None, None, None, None, true)
            .unwrap(),
        quotes,
    );
}

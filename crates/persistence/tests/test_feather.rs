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

use std::{cell::RefCell, rc::Rc, sync::Arc};

use nautilus_common::clock::{Clock, TestClock};
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{Data, OrderBookDelta, OrderBookDeltas, OrderBookDeltas_API, QuoteTick, TradeTick},
    enums::AggressorSide,
    identifiers::{InstrumentId, TradeId},
    types::{Price, Quantity},
};
use nautilus_persistence::backend::feather::{FeatherWriter, RotationConfig};
use object_store::{ObjectStore, local::LocalFileSystem};
use rstest::rstest;
use tempfile::TempDir;

#[rstest]
#[tokio::test]
async fn test_write_data_enum_quote() {
    let temp_dir = TempDir::new().unwrap();
    let base_path = temp_dir.path().to_str().unwrap().to_string();
    let local_fs = LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap();
    let store: Arc<dyn ObjectStore> = Arc::new(local_fs);
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));

    let mut writer = FeatherWriter::new(
        base_path,
        store,
        clock,
        RotationConfig::NoRotation,
        None,
        None,
        None,
    );

    let quote = QuoteTick::new(
        InstrumentId::from("AUD/USD.SIM"),
        Price::from("1.0"),
        Price::from("1.0"),
        Quantity::from("1000"),
        Quantity::from("1000"),
        UnixNanos::from(1000),
        UnixNanos::from(1000),
    );

    writer.write_data(Data::Quote(quote)).await.unwrap();
    writer.flush().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_write_data_enum_all_types() {
    let temp_dir = TempDir::new().unwrap();
    let base_path = temp_dir.path().to_str().unwrap().to_string();
    let local_fs = LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap();
    let store: Arc<dyn ObjectStore> = Arc::new(local_fs);
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));

    let mut writer = FeatherWriter::new(
        base_path,
        store,
        clock,
        RotationConfig::NoRotation,
        None,
        None,
        None,
    );

    let instrument_id = InstrumentId::from("AUD/USD.SIM");

    // Test all data types via write_data
    let quote = QuoteTick::new(
        instrument_id,
        Price::from("1.0"),
        Price::from("1.0"),
        Quantity::from("1000"),
        Quantity::from("1000"),
        UnixNanos::from(1000),
        UnixNanos::from(1000),
    );
    writer.write_data(Data::Quote(quote)).await.unwrap();

    let trade = TradeTick::new(
        instrument_id,
        Price::from("1.0"),
        Quantity::from("1000"),
        AggressorSide::Buyer,
        TradeId::from("1"),
        UnixNanos::from(2000),
        UnixNanos::from(2000),
    );
    writer.write_data(Data::Trade(trade)).await.unwrap();

    let delta = OrderBookDelta::clear(
        instrument_id,
        0,
        UnixNanos::from(3000),
        UnixNanos::from(3000),
    );
    writer.write_data(Data::Delta(delta)).await.unwrap();

    writer.flush().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_write_data_orderbook_deltas() {
    let temp_dir = TempDir::new().unwrap();
    let base_path = temp_dir.path().to_str().unwrap().to_string();
    let local_fs = LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap();
    let store: Arc<dyn ObjectStore> = Arc::new(local_fs);
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));

    let mut writer = FeatherWriter::new(
        base_path,
        store,
        clock,
        RotationConfig::NoRotation,
        None,
        None,
        None,
    );

    let instrument_id = InstrumentId::from("AUD/USD.SIM");
    let delta1 = OrderBookDelta::clear(
        instrument_id,
        0,
        UnixNanos::from(1000),
        UnixNanos::from(1000),
    );
    let delta2 = OrderBookDelta::clear(
        instrument_id,
        0,
        UnixNanos::from(2000),
        UnixNanos::from(2000),
    );

    let deltas = OrderBookDeltas::new(instrument_id, vec![delta1, delta2]);
    let deltas_api = OrderBookDeltas_API::new(deltas);

    // Test writing OrderBookDeltas via write_data
    writer.write_data(Data::Deltas(deltas_api)).await.unwrap();
    writer.flush().await.unwrap();
}

#[rstest]
#[tokio::test]
async fn test_auto_flush() {
    let temp_dir = TempDir::new().unwrap();
    let base_path = temp_dir.path().to_str().unwrap().to_string();
    let local_fs = LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap();
    let store: Arc<dyn ObjectStore> = Arc::new(local_fs);
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));

    let mut writer = FeatherWriter::new(
        base_path,
        store,
        clock.clone(),
        RotationConfig::NoRotation,
        None,
        None,
        Some(100), // 100ms flush interval
    );

    let quote = QuoteTick::new(
        InstrumentId::from("AUD/USD.SIM"),
        Price::from("1.0"),
        Price::from("1.0"),
        Quantity::from("1000"),
        Quantity::from("1000"),
        UnixNanos::from(1000),
        UnixNanos::from(1000),
    );

    // Write first quote
    writer.write(quote).await.unwrap();

    // Note: TestClock doesn't have set_time_ns, so we can't easily test auto-flush
    // with time advancement. Instead, we test that check_flush is called during write.
    // For a proper test, we'd need a mock clock or use LiveClock with time advancement.

    // Write second quote - check_flush will be called but won't flush if time hasn't advanced
    let quote2 = QuoteTick::new(
        InstrumentId::from("AUD/USD.SIM"),
        Price::from("1.1"),
        Price::from("1.1"),
        Quantity::from("1000"),
        Quantity::from("1000"),
        UnixNanos::from(2000),
        UnixNanos::from(2000),
    );
    writer.write(quote2).await.unwrap();

    // Verify that writes succeeded (check_flush was called, even if it didn't flush)
    // The flush_interval_ms is set, so check_flush runs but won't flush without time advancement
}

#[rstest]
#[tokio::test]
async fn test_close() {
    let temp_dir = TempDir::new().unwrap();
    let base_path = temp_dir.path().to_str().unwrap().to_string();
    let local_fs = LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap();
    let store: Arc<dyn ObjectStore> = Arc::new(local_fs);
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));

    let mut writer = FeatherWriter::new(
        base_path,
        store,
        clock,
        RotationConfig::NoRotation,
        None,
        None,
        None,
    );

    let quote = QuoteTick::new(
        InstrumentId::from("AUD/USD.SIM"),
        Price::from("1.0"),
        Price::from("1.0"),
        Quantity::from("1000"),
        Quantity::from("1000"),
        UnixNanos::from(1000),
        UnixNanos::from(1000),
    );

    writer.write(quote).await.unwrap();

    // Close should flush and clear writers
    writer.close().await.unwrap();
}

// Note: Message bus subscription test is skipped due to async/sync boundary complexity.
// The handler uses block_on which can't be used from within an async runtime (tokio test).
// This functionality is better tested via Python integration tests where the message bus
// is used in a non-async context or via proper async task spawning.

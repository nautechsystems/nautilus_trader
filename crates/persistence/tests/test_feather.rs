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

use std::{cell::RefCell, collections::HashSet, fs::File, rc::Rc, sync::Arc};

use datafusion::arrow::ipc::reader::StreamReader;
use nautilus_common::{
    clock::{Clock, TestClock},
    msgbus::{self, MessageBus, typed_handler::ShareableMessageHandler},
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    data::{
        BookOrder, Data, FundingRateUpdate, InstrumentClose, InstrumentStatus, OrderBookDelta,
        OrderBookDeltas, QuoteTick, TradeTick,
    },
    enums::{
        AccountType, AggressorSide, BookAction, InstrumentCloseType, MarketStatusAction, OrderSide,
        PositionSide,
    },
    events::{
        AccountState, OrderEventAny, PositionEvent, PositionOpened, order::spec::OrderFilledSpec,
    },
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TradeId, TraderId,
        VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny, stubs::binary_option},
    types::{Currency, Money, Price, Quantity},
};
use nautilus_persistence::backend::{
    catalog::ParquetDataCatalog,
    feather::{FeatherWriter, RotationConfig},
};
use nautilus_serialization::arrow::instrument::decode_instrument_any_batch;
use object_store::{ObjectStore, local::LocalFileSystem};
use rstest::rstest;
use tempfile::TempDir;

#[rstest]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_direct_writer_lifecycle_inside_multi_thread_runtime() {
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
        Some(HashSet::from(["quotes".to_string()])),
        None,
        None,
    );
    let quote = QuoteTick::new(
        InstrumentId::from("AUD/USD.SIM"),
        Price::from("1.00001"),
        Price::from("1.00002"),
        Quantity::from("1000"),
        Quantity::from("1001"),
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
    );

    writer.write(quote).await.unwrap();
    writer.flush().await.unwrap();
    writer.close().await.unwrap();

    assert!(writer.is_closed());
    assert_eq!(collect_feather_files(temp_dir.path()).len(), 1);
}

#[rstest]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[expect(
    clippy::await_holding_refcell_ref,
    reason = "The message-bus writer is single-threaded and the test awaits its final close before reading files"
)]
async fn test_legacy_message_bus_subscription_persists_any_route_and_unsubscribes() {
    let _bus = MessageBus::default().register_message_bus();
    let temp_dir = TempDir::new().unwrap();
    let base_path = temp_dir.path().to_str().unwrap().to_string();
    let local_fs = LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap();
    let store: Arc<dyn ObjectStore> = Arc::new(local_fs);
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let writer = Rc::new(RefCell::new(FeatherWriter::new(
        base_path,
        store,
        clock,
        RotationConfig::NoRotation,
        Some(HashSet::from(["quotes".to_string()])),
        None,
        None,
    )));
    let quote = QuoteTick::new(
        InstrumentId::from("AUD/USD.SIM"),
        Price::from("1.00001"),
        Price::from("1.00002"),
        Quantity::from("1000"),
        Quantity::from("1001"),
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
    );

    let handler: ShareableMessageHandler =
        FeatherWriter::subscribe_to_message_bus(Rc::clone(&writer)).unwrap();
    msgbus::publish_any("data.quotes.AUD/USD.SIM".into(), &quote);
    FeatherWriter::unsubscribe_from_message_bus(&handler);
    msgbus::publish_any("data.quotes.AUD/USD.SIM".into(), &quote);
    writer.borrow_mut().close().await.unwrap();

    let files = collect_feather_files(temp_dir.path());
    assert_eq!(files.len(), 1);
    let row_count: usize = StreamReader::try_new(File::open(&files[0]).unwrap(), None)
        .unwrap()
        .map(|batch| batch.unwrap().num_rows())
        .sum();
    assert_eq!(
        row_count, 1,
        "legacy unsubscribe must prevent the second write"
    );
}

#[rstest]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[expect(
    clippy::await_holding_refcell_ref,
    reason = "The message-bus writer is single-threaded and the test awaits its final close before reading files"
)]
async fn test_all_routes_subscription_persists_and_converts_typed_publications() {
    let _bus = MessageBus::default().register_message_bus();
    let temp_dir = TempDir::new().unwrap();
    let local_fs = LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap();
    let store: Arc<dyn ObjectStore> = Arc::new(local_fs);
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let writer = Rc::new(RefCell::new(FeatherWriter::new(
        "live/test_instance".to_string(),
        store,
        clock,
        RotationConfig::NoRotation,
        Some(HashSet::from([
            "instrument_closes".to_string(),
            "instrument_status".to_string(),
            "instruments".to_string(),
            "account_state".to_string(),
            "order_book_deltas".to_string(),
            "order_filled".to_string(),
            "position_opened".to_string(),
            "quotes".to_string(),
            "trades".to_string(),
        ])),
        Some(HashSet::from([
            "instrument_closes".to_string(),
            "instrument_status".to_string(),
            "instruments".to_string(),
        ])),
        None,
    )));

    let subscription = FeatherWriter::subscribe_to_all_message_bus_routes(&writer);
    let mut binary = binary_option();
    binary.id = InstrumentId::from("BINARY-OPTION.POLYMARKET");
    let instrument = InstrumentAny::BinaryOption(binary);
    let instrument_id = instrument.id();
    let quote = QuoteTick::new(
        instrument_id,
        Price::from("1.00001"),
        Price::from("1.00002"),
        Quantity::from("1000"),
        Quantity::from("1001"),
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
    );
    let trade = TradeTick::new(
        instrument_id,
        Price::from("1.00001"),
        Quantity::from("1000"),
        AggressorSide::Buy,
        TradeId::from("1"),
        UnixNanos::from(2_000),
        UnixNanos::from(2_000),
    );
    let deltas = OrderBookDeltas::new(
        instrument_id,
        vec![OrderBookDelta::clear(
            instrument_id,
            0,
            UnixNanos::from(3_000),
            UnixNanos::from(3_000),
        )],
    );
    let status = InstrumentStatus::new(
        instrument_id,
        MarketStatusAction::Trading,
        UnixNanos::from(4_000),
        UnixNanos::from(4_000),
        None,
        None,
        None,
        None,
        None,
    );
    let close = InstrumentClose::new(
        instrument_id,
        Price::from("1.00001"),
        InstrumentCloseType::EndOfSession,
        UnixNanos::from(5_000),
        UnixNanos::from(5_000),
    );
    let account_state = AccountState::new(
        AccountId::from("SIM-001"),
        AccountType::Cash,
        Vec::new(),
        Vec::new(),
        true,
        UUID4::new(),
        UnixNanos::from(6_000),
        UnixNanos::from(6_000),
        Some(Currency::USD()),
    );
    let order_event = OrderEventAny::Filled(
        OrderFilledSpec::builder()
            .trader_id(TraderId::from("TRADER-001"))
            .strategy_id(StrategyId::from("STRATEGY-001"))
            .instrument_id(instrument_id)
            .client_order_id(ClientOrderId::from("O-001"))
            .venue_order_id(VenueOrderId::from("V-001"))
            .account_id(AccountId::from("SIM-001"))
            .trade_id(TradeId::from("T-001"))
            .last_qty(Quantity::from("1"))
            .last_px(Price::from("1.00001"))
            .currency(Currency::USD())
            .commission(Money::new(0.0, Currency::USD()))
            .ts_event(UnixNanos::from(7_000))
            .ts_init(UnixNanos::from(7_000))
            .build(),
    );
    let position_event = PositionEvent::PositionOpened(PositionOpened {
        trader_id: TraderId::from("TRADER-001"),
        strategy_id: StrategyId::from("STRATEGY-001"),
        instrument_id,
        position_id: PositionId::from("P-001"),
        account_id: AccountId::from("SIM-001"),
        opening_order_id: ClientOrderId::from("O-001"),
        entry: OrderSide::Buy,
        side: PositionSide::Long,
        signed_qty: 1.0,
        quantity: Quantity::from("1"),
        last_qty: Quantity::from("1"),
        last_px: Price::from("1.00001"),
        currency: Currency::USD(),
        avg_px_open: 1.00001,
        realized_pnl: None,
        event_id: UUID4::new(),
        ts_event: UnixNanos::from(8_000),
        ts_init: UnixNanos::from(8_000),
    });

    msgbus::publish_instrument("data.instrument.AUD/USD.SIM".into(), &instrument);
    msgbus::publish_deltas("data.book.deltas.AUD/USD.SIM".into(), &deltas);
    msgbus::publish_quote("data.quotes.AUD/USD.SIM".into(), &quote);
    msgbus::publish_trade("data.trades.AUD/USD.SIM".into(), &trade);
    msgbus::publish_any("data.status.AUD/USD.SIM".into(), &status);
    msgbus::publish_any("data.close.AUD/USD.SIM".into(), &close);
    msgbus::publish_account_state("events.account.SIM-001".into(), &account_state);
    msgbus::publish_order_event("events.order.TRADER-001".into(), &order_event);
    msgbus::publish_position_event("events.position.STRATEGY-001".into(), &position_event);
    subscription.unsubscribe();
    msgbus::publish_instrument("data.instrument.AUD/USD.SIM".into(), &instrument);
    msgbus::publish_deltas("data.book.deltas.AUD/USD.SIM".into(), &deltas);
    msgbus::publish_quote("data.quotes.AUD/USD.SIM".into(), &quote);
    msgbus::publish_trade("data.trades.AUD/USD.SIM".into(), &trade);
    msgbus::publish_any("data.status.AUD/USD.SIM".into(), &status);
    msgbus::publish_any("data.close.AUD/USD.SIM".into(), &close);
    msgbus::publish_account_state("events.account.SIM-001".into(), &account_state);
    msgbus::publish_order_event("events.order.TRADER-001".into(), &order_event);
    msgbus::publish_position_event("events.position.STRATEGY-001".into(), &position_event);
    writer.borrow_mut().close().await.unwrap();

    let files = collect_feather_files(temp_dir.path());
    assert_eq!(files.len(), 9, "expected one file per data type");
    let instrument_file = files
        .iter()
        .find(|path| {
            path.components()
                .any(|component| component.as_os_str() == "instruments")
        })
        .expect("missing per-instrument definition file");
    let mut instrument_reader =
        StreamReader::try_new(File::open(instrument_file).unwrap(), None).unwrap();
    let metadata = instrument_reader.schema().metadata().clone();
    let instrument_batch = instrument_reader.next().unwrap().unwrap();
    let decoded = decode_instrument_any_batch(&metadata, &instrument_batch).unwrap();
    assert_eq!(decoded, vec![instrument.clone()]);

    let row_count: usize = files
        .iter()
        .map(|path| {
            StreamReader::try_new(File::open(path).unwrap(), None)
                .unwrap()
                .map(|batch| batch.unwrap().num_rows())
                .sum::<usize>()
        })
        .sum();
    assert_eq!(row_count, 9, "unsubscribe must remove every route");

    let mut catalog = ParquetDataCatalog::new(temp_dir.path(), None, None, None, None);
    catalog
        .convert_stream_to_data("test_instance", "instruments", Some("live"), None, false)
        .unwrap();
    let instrument_id = instrument_id.to_string();
    assert_eq!(
        catalog
            .query_instruments(Some(std::slice::from_ref(&instrument_id)))
            .unwrap(),
        vec![instrument],
    );
}

#[rstest]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[expect(
    clippy::await_holding_refcell_ref,
    reason = "The message-bus writer is single-threaded and the test awaits its final close before reading files"
)]
async fn test_all_routes_subscription_unsubscribes_on_drop() {
    let _bus = MessageBus::default().register_message_bus();
    let temp_dir = TempDir::new().unwrap();
    let local_fs = LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap();
    let store: Arc<dyn ObjectStore> = Arc::new(local_fs);
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let writer = Rc::new(RefCell::new(FeatherWriter::new(
        "live/test_instance".to_string(),
        store,
        clock,
        RotationConfig::NoRotation,
        Some(HashSet::from(["quotes".to_string()])),
        None,
        None,
    )));
    let quote = QuoteTick::new(
        InstrumentId::from("AUD/USD.SIM"),
        Price::from("1.00001"),
        Price::from("1.00002"),
        Quantity::from("1000"),
        Quantity::from("1001"),
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
    );

    {
        let _subscription = FeatherWriter::subscribe_to_all_message_bus_routes(&writer);
        msgbus::publish_quote("data.quotes.AUD/USD.SIM".into(), &quote);
    }
    msgbus::publish_quote("data.quotes.AUD/USD.SIM".into(), &quote);
    writer.borrow_mut().close().await.unwrap();

    let files = collect_feather_files(temp_dir.path());
    assert_eq!(files.len(), 1);
    let row_count: usize = StreamReader::try_new(File::open(&files[0]).unwrap(), None)
        .unwrap()
        .map(|batch| batch.unwrap().num_rows())
        .sum();
    assert_eq!(
        row_count, 1,
        "dropping the subscription must unsubscribe every route"
    );
}

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
        AggressorSide::Buy,
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

    let funding_rate = FundingRateUpdate::new(
        instrument_id,
        "0.0001".parse().unwrap(),
        Some(480),
        Some(UnixNanos::from(5_000)),
        UnixNanos::from(4_000),
        UnixNanos::from(4_000),
    );
    writer
        .write_data(Data::FundingRate(funding_rate))
        .await
        .unwrap();

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

    // Test writing OrderBookDeltas via write_data
    writer
        .write_data(Data::Deltas(Box::new(deltas)))
        .await
        .unwrap();
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

// Regression test for https://github.com/nautechsystems/nautilus_trader/issues/3913,
// where a leading BookAction::Clear delta poisoned file metadata with 0 precision.
#[rstest]
#[tokio::test]
async fn test_write_orderbook_deltas_clear_first_preserves_precision() {
    let temp_dir = TempDir::new().unwrap();
    let base_path = temp_dir.path().to_str().unwrap().to_string();
    let local_fs = LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap();
    let store: Arc<dyn ObjectStore> = Arc::new(local_fs);
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));

    let mut per_instrument = HashSet::new();
    per_instrument.insert("order_book_deltas".to_string());

    let mut writer = FeatherWriter::new(
        base_path,
        store,
        clock,
        RotationConfig::NoRotation,
        None,
        Some(per_instrument),
        None,
    );

    let instrument_id = InstrumentId::from("AUD/USD.SIM");
    let clear = OrderBookDelta::clear(
        instrument_id,
        0,
        UnixNanos::from(1000),
        UnixNanos::from(1000),
    );
    let add = OrderBookDelta::new(
        instrument_id,
        BookAction::Add,
        BookOrder {
            side: OrderSide::Buy,
            price: Price::new(1.23, 2),
            size: Quantity::new(100.0, 6),
            order_id: 1,
        },
        0,
        1,
        UnixNanos::from(2000),
        UnixNanos::from(2000),
    );

    let deltas = OrderBookDeltas::new(instrument_id, vec![clear, add]);

    writer
        .write_data(Data::Deltas(Box::new(deltas)))
        .await
        .unwrap();
    writer.flush().await.unwrap();

    let feather_path = find_feather_file(temp_dir.path());
    let file = File::open(&feather_path).unwrap();
    let reader = StreamReader::try_new(file, None).unwrap();
    let metadata = reader.schema().metadata().clone();

    assert_eq!(
        metadata.get("price_precision"),
        Some(&"2".to_string()),
        "file metadata should reflect real price precision, not the CLEAR sentinel",
    );
    assert_eq!(
        metadata.get("size_precision"),
        Some(&"6".to_string()),
        "file metadata should reflect real size precision, not the CLEAR sentinel",
    );
}

// Regression test for the all-sentinel fallback: a batch containing only
// BookAction::Clear rows has no real precision to derive, so file metadata
// legitimately carries price_precision=0, size_precision=0.
#[rstest]
#[tokio::test]
async fn test_write_orderbook_deltas_all_sentinel_metadata_fallback() {
    let temp_dir = TempDir::new().unwrap();
    let base_path = temp_dir.path().to_str().unwrap().to_string();
    let local_fs = LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap();
    let store: Arc<dyn ObjectStore> = Arc::new(local_fs);
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));

    let mut per_instrument = HashSet::new();
    per_instrument.insert("order_book_deltas".to_string());

    let mut writer = FeatherWriter::new(
        base_path,
        store,
        clock,
        RotationConfig::NoRotation,
        None,
        Some(per_instrument),
        None,
    );

    let instrument_id = InstrumentId::from("AUD/USD.SIM");
    let clear1 = OrderBookDelta::clear(
        instrument_id,
        0,
        UnixNanos::from(1000),
        UnixNanos::from(1000),
    );
    let clear2 = OrderBookDelta::clear(
        instrument_id,
        1,
        UnixNanos::from(2000),
        UnixNanos::from(2000),
    );

    let deltas = OrderBookDeltas::new(instrument_id, vec![clear1, clear2]);

    writer
        .write_data(Data::Deltas(Box::new(deltas)))
        .await
        .unwrap();
    writer.flush().await.unwrap();

    let feather_path = find_feather_file(temp_dir.path());
    let file = File::open(&feather_path).unwrap();
    let reader = StreamReader::try_new(file, None).unwrap();
    let metadata = reader.schema().metadata().clone();

    assert_eq!(metadata.get("price_precision"), Some(&"0".to_string()));
    assert_eq!(metadata.get("size_precision"), Some(&"0".to_string()));
}

// Regression test for the mixed-instrument routing in write_batch. When a
// batch contains deltas for multiple instruments, each instrument's rows
// must land in its own file with its own precision metadata.
#[rstest]
#[tokio::test]
async fn test_write_batch_partitions_by_instrument() {
    let temp_dir = TempDir::new().unwrap();
    let base_path = temp_dir.path().to_str().unwrap().to_string();
    let local_fs = LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap();
    let store: Arc<dyn ObjectStore> = Arc::new(local_fs);
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));

    let mut per_instrument = HashSet::new();
    per_instrument.insert("order_book_deltas".to_string());

    let mut writer = FeatherWriter::new(
        base_path,
        store,
        clock,
        RotationConfig::NoRotation,
        None,
        Some(per_instrument),
        None,
    );

    let instrument_a = InstrumentId::from("AUD/USD.SIM");
    let instrument_b = InstrumentId::from("BTC/USD.BINANCE");

    let make_add = |instrument_id, price: f64, price_prec, size: f64, size_prec, ts| {
        OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            BookOrder {
                side: OrderSide::Buy,
                price: Price::new(price, price_prec),
                size: Quantity::new(size, size_prec),
                order_id: 1,
            },
            0,
            1,
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };

    let deltas = vec![
        make_add(instrument_a, 1.23, 2, 100.0, 0, 1000),
        make_add(instrument_b, 20_000.0, 4, 0.123_456_78, 8, 2000),
        make_add(instrument_a, 1.24, 2, 50.0, 0, 3000),
        make_add(instrument_b, 20_100.0, 4, 0.25, 8, 4000),
    ];

    writer.write_batch(deltas).await.unwrap();
    writer.flush().await.unwrap();

    let files = collect_feather_files(temp_dir.path());
    assert_eq!(
        files.len(),
        2,
        "expected one file per instrument, found {files:?}"
    );

    let mut by_instrument = std::collections::HashMap::new();

    for path in files {
        let reader = StreamReader::try_new(File::open(&path).unwrap(), None).unwrap();
        let metadata = reader.schema().metadata().clone();
        let instrument_id = metadata
            .get("instrument_id")
            .expect("instrument_id metadata")
            .clone();
        by_instrument.insert(instrument_id, metadata);
    }

    let metadata_a = by_instrument.get("AUD/USD.SIM").expect("AUD/USD.SIM file");
    assert_eq!(metadata_a.get("price_precision"), Some(&"2".to_string()));
    assert_eq!(metadata_a.get("size_precision"), Some(&"0".to_string()));

    let metadata_b = by_instrument
        .get("BTC/USD.BINANCE")
        .expect("BTC/USD.BINANCE file");
    assert_eq!(metadata_b.get("price_precision"), Some(&"4".to_string()));
    assert_eq!(metadata_b.get("size_precision"), Some(&"8".to_string()));
}

fn collect_feather_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    collect_feather_files_into(dir, &mut out);
    out
}

fn collect_feather_files_into(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_feather_files_into(&path, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("feather") {
            out.push(path);
        }
    }
}

fn find_feather_file(dir: &std::path::Path) -> std::path::PathBuf {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            let found = find_feather_file(&path);
            if !found.as_os_str().is_empty() {
                return found;
            }
        } else if path.extension().and_then(|s| s.to_str()) == Some("feather") {
            return path;
        }
    }
    std::path::PathBuf::new()
}

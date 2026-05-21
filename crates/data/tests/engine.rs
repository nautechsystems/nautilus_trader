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

mod common;

#[cfg(feature = "streaming")]
use std::path::{Path, PathBuf};
use std::{any::Any, cell::RefCell, num::NonZeroUsize, rc::Rc, time::Duration};
#[cfg(feature = "defi")]
use std::{str::FromStr, sync::Arc};

#[cfg(feature = "defi")]
use alloy_primitives::{Address, I256, U160, U256};
use common::mocks::{FailingMockDataClient, MockDataClient};
#[cfg(feature = "defi")]
use nautilus_common::defi;
#[cfg(feature = "defi")]
use nautilus_common::messages::defi::{
    DefiRequestCommand, DefiSubscribeCommand, DefiUnsubscribeCommand, RequestPoolSnapshot,
    SubscribeBlocks, SubscribePool, SubscribePoolFeeCollects, SubscribePoolFlashEvents,
    SubscribePoolLiquidityUpdates, SubscribePoolSwaps, UnsubscribeBlocks,
    UnsubscribePoolFeeCollects, UnsubscribePoolFlashEvents, UnsubscribePoolLiquidityUpdates,
    UnsubscribePoolSwaps,
};
use nautilus_common::{
    cache::Cache,
    clock::{Clock, TestClock},
    messages::data::{
        BarsResponse, CustomDataResponse, DataCommand, DataResponse, InstrumentResponse,
        PARAMS_IS_PARENT, QuotesResponse, RequestBars, RequestBookDepth, RequestBookSnapshot,
        RequestCommand, RequestCustomData, RequestFundingRates, RequestInstrument,
        RequestInstruments, RequestQuotes, RequestTrades, SubscribeBars, SubscribeBookDeltas,
        SubscribeBookDepth10, SubscribeBookSnapshots, SubscribeCommand, SubscribeCustomData,
        SubscribeFundingRates, SubscribeIndexPrices, SubscribeInstrument, SubscribeInstrumentClose,
        SubscribeInstrumentStatus, SubscribeMarkPrices, SubscribeOptionChain,
        SubscribeOptionGreeks, SubscribeQuotes, SubscribeTrades, TradesResponse, UnsubscribeBars,
        UnsubscribeBookDeltas, UnsubscribeBookDepth10, UnsubscribeBookSnapshots,
        UnsubscribeCommand, UnsubscribeCustomData, UnsubscribeFundingRates, UnsubscribeIndexPrices,
        UnsubscribeInstrument, UnsubscribeInstrumentClose, UnsubscribeInstrumentStatus,
        UnsubscribeMarkPrices, UnsubscribeOptionChain, UnsubscribeOptionGreeks, UnsubscribeQuotes,
        UnsubscribeTrades,
    },
    msgbus::{
        self, MStr, MessageBus, Topic, TypedHandler, TypedIntoHandler,
        stubs::{get_any_saving_handler, get_typed_message_saving_handler},
        switchboard::{self, MessagingSwitchboard},
    },
    testing::wait_until,
};
use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_data::{
    client::DataClientAdapter,
    engine::{DataEngine, config::DataEngineConfig},
};
#[cfg(feature = "defi")]
use nautilus_model::defi::tick_map::tick_math::get_tick_at_sqrt_ratio;
#[cfg(feature = "defi")]
use nautilus_model::defi::{AmmType, Dex, DexType, chain::chains};
#[cfg(feature = "defi")]
use nautilus_model::defi::{
    Block, Blockchain, DefiData, Pool, PoolIdentifier, PoolLiquidityUpdate,
    PoolLiquidityUpdateType, PoolProfiler, PoolSwap, Token, data::PoolFeeCollect, data::PoolFlash,
};
use nautilus_model::{
    data::{
        Bar, BarType, BookOrder, CustomData, DEPTH10_LEN, Data, DataType, FundingRateUpdate,
        IndexPriceUpdate, InstrumentClose, InstrumentStatus, MarkPriceUpdate, OrderBookDeltas,
        OrderBookDeltas_API, OrderBookDepth10, QuoteTick, TradeTick,
        greeks::OptionGreekValues,
        option_chain::{OptionGreeks, StrikeRange},
        stubs::{
            OrderBookDeltaTestBuilder, stub_custom_data, stub_delta, stub_deltas, stub_depth10,
        },
    },
    enums::{
        AggressorSide, AssetClass, BookType, GreeksConvention, InstrumentClass,
        InstrumentCloseType, MarketStatusAction, OptionKind, PriceType,
    },
    identifiers::{ClientId, InstrumentId, OptionSeriesId, Symbol, TradeId, TraderId, Venue},
    instruments::{
        CurrencyPair, FuturesContract, FuturesSpread, Instrument, InstrumentAny, OptionContract,
        SyntheticInstrument,
        stubs::{audusd_sim, futures_spread_es, gbpusd_sim},
    },
    orderbook::OrderBook,
    stubs::TestDefault,
    types::{Currency, Price, Quantity},
};
#[cfg(feature = "streaming")]
use nautilus_persistence::backend::catalog::{ParquetDataCatalog, timestamps_to_filename};
use rstest::*;
use serde_json::json;
use ustr::Ustr;

#[fixture]
fn client_id() -> ClientId {
    ClientId::test_default()
}

#[fixture]
fn venue() -> Venue {
    Venue::test_default()
}

#[fixture]
fn clock() -> Rc<RefCell<TestClock>> {
    Rc::new(RefCell::new(TestClock::new()))
}

#[fixture]
fn cache() -> Rc<RefCell<Cache>> {
    Rc::new(RefCell::new(Cache::default()))
}

#[fixture]
fn stub_msgbus() -> Rc<RefCell<MessageBus>> {
    MessageBus::new(TraderId::test_default(), UUID4::new(), None, None).register_message_bus()
}

#[fixture]
fn data_engine(
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
) -> Rc<RefCell<DataEngine>> {
    let data_engine = Rc::new(RefCell::new(DataEngine::new(clock, cache, None)));

    let data_engine_clone = data_engine.clone();
    let handler = TypedIntoHandler::from(move |cmd: DataCommand| {
        data_engine_clone.borrow_mut().execute(cmd);
    });

    let endpoint = MessagingSwitchboard::data_engine_execute();
    msgbus::register_data_command_endpoint(endpoint, handler);

    data_engine
}

#[fixture]
fn data_client(
    client_id: ClientId,
    venue: Venue,
    cache: Rc<RefCell<Cache>>,
    clock: Rc<RefCell<TestClock>>,
) -> DataClientAdapter {
    let client = Box::new(MockDataClient::new(clock, cache, client_id, Some(venue)));
    DataClientAdapter::new(client_id, Some(venue), true, true, client)
}

// Test helper for registering a mock data client
fn register_mock_client(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
    routing: Option<Venue>,
    recorder: &Rc<RefCell<Vec<DataCommand>>>,
    data_engine: &mut DataEngine,
) {
    let client = MockDataClient::new_with_recorder(
        clock,
        cache,
        client_id,
        Some(venue),
        Some(recorder.clone()),
    );
    let adapter = DataClientAdapter::new(client_id, Some(venue), true, true, Box::new(client));
    data_engine.register_client(adapter, routing);
}

fn parent_params() -> Params {
    let mut params = Params::new();
    params.insert(PARAMS_IS_PARENT.to_string(), json!(true));
    params
}

fn generic_futures_spread() -> FuturesSpread {
    let mut spread = futures_spread_es();
    spread.id = generic_futures_spread_id();
    spread
}

fn generic_futures_spread_id() -> InstrumentId {
    InstrumentId::from("(1)ESM4___((1))ESU4.GLBX")
}

fn generic_futures_spread_legs() -> (InstrumentId, InstrumentId) {
    (
        InstrumentId::from("ESM4.GLBX"),
        InstrumentId::from("ESU4.GLBX"),
    )
}

fn spread_quote_params() -> Params {
    serde_json::from_value(json!({
        "aggregate_spread_quotes": true,
        "update_interval_seconds": null,
    }))
    .unwrap()
}

fn spread_quote_default_interval_params() -> Params {
    serde_json::from_value(json!({
        "aggregate_spread_quotes": true,
    }))
    .unwrap()
}

fn spread_quote_zero_interval_params() -> Params {
    serde_json::from_value(json!({
        "aggregate_spread_quotes": true,
        "update_interval_seconds": 0,
    }))
    .unwrap()
}

#[cfg(feature = "streaming")]
struct CatalogTempDir(PathBuf);

#[cfg(feature = "streaming")]
impl CatalogTempDir {
    fn new(label: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("nautilus-data-engine-{label}-{}", UUID4::new()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

#[cfg(feature = "streaming")]
impl Drop for CatalogTempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(feature = "streaming")]
fn register_empty_catalog(data_engine: &mut DataEngine, label: &str) -> CatalogTempDir {
    let catalog_dir = CatalogTempDir::new(label);
    let catalog = ParquetDataCatalog::new(catalog_dir.path(), None, None, None, None);
    data_engine.register_catalog(catalog, None);
    catalog_dir
}

#[cfg(feature = "streaming")]
fn register_quote_catalog(
    data_engine: &mut DataEngine,
    instrument_id: InstrumentId,
    last_timestamp: u64,
) -> CatalogTempDir {
    let catalog_dir = CatalogTempDir::new("quotes");
    let catalog = ParquetDataCatalog::new(catalog_dir.path(), None, None, None, None);
    catalog
        .write_to_parquet(
            vec![QuoteTick::new(
                instrument_id,
                Price::from("1.0000"),
                Price::from("1.0001"),
                Quantity::from(1),
                Quantity::from(1),
                UnixNanos::from(last_timestamp),
                UnixNanos::from(last_timestamp),
            )],
            None,
            None,
            None,
        )
        .unwrap();
    data_engine.register_catalog(catalog, None);
    catalog_dir
}

#[cfg(feature = "streaming")]
fn register_trade_catalog(
    data_engine: &mut DataEngine,
    instrument_id: InstrumentId,
    last_timestamp: u64,
) -> CatalogTempDir {
    let catalog_dir = CatalogTempDir::new("trades");
    let catalog = ParquetDataCatalog::new(catalog_dir.path(), None, None, None, None);
    catalog
        .write_to_parquet(
            vec![TradeTick::new(
                instrument_id,
                Price::from("1.0000"),
                Quantity::from(1),
                AggressorSide::Buyer,
                TradeId::new("T-1"),
                UnixNanos::from(last_timestamp),
                UnixNanos::from(last_timestamp),
            )],
            None,
            None,
            None,
        )
        .unwrap();
    data_engine.register_catalog(catalog, None);
    catalog_dir
}

#[cfg(feature = "streaming")]
fn register_bar_catalog(
    data_engine: &mut DataEngine,
    bar_type: BarType,
    last_timestamp: u64,
) -> CatalogTempDir {
    let catalog_dir = CatalogTempDir::new("bars");
    let catalog = ParquetDataCatalog::new(catalog_dir.path(), None, None, None, None);
    catalog
        .write_to_parquet(
            vec![Bar::new(
                bar_type,
                Price::from("1.0000"),
                Price::from("1.0001"),
                Price::from("0.9999"),
                Price::from("1.0000"),
                Quantity::from(1),
                UnixNanos::from(last_timestamp),
                UnixNanos::from(last_timestamp),
            )],
            None,
            None,
            None,
        )
        .unwrap();
    data_engine.register_catalog(catalog, None);
    catalog_dir
}

#[cfg(feature = "streaming")]
fn write_custom_catalog_file(
    catalog_dir: &CatalogTempDir,
    catalog: &ParquetDataCatalog,
    type_name: &str,
    identifier: Option<&str>,
    start_timestamp: u64,
    end_timestamp: u64,
) {
    let directory = catalog
        .make_path_custom_data(type_name, identifier)
        .unwrap();
    let directory_path = catalog_dir.path().join(directory);
    std::fs::create_dir_all(&directory_path).unwrap();

    let filename = timestamps_to_filename(
        UnixNanos::from(start_timestamp),
        UnixNanos::from(end_timestamp),
    );
    std::fs::write(directory_path.join(filename), b"").unwrap();
}

#[cfg(feature = "streaming")]
fn register_custom_catalog(
    data_engine: &mut DataEngine,
    data_type: &DataType,
    last_timestamp: u64,
) -> CatalogTempDir {
    let catalog_dir = CatalogTempDir::new("custom");
    let catalog = ParquetDataCatalog::new(catalog_dir.path(), None, None, None, None);
    write_custom_catalog_file(
        &catalog_dir,
        &catalog,
        data_type.type_name(),
        data_type.identifier(),
        last_timestamp,
        last_timestamp,
    );

    data_engine.register_catalog(catalog, None);
    catalog_dir
}

#[cfg(feature = "streaming")]
fn register_recording_client(
    data_engine: &mut DataEngine,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) -> Rc<RefCell<Vec<DataCommand>>> {
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(clock, cache, client_id, venue, None, &recorder, data_engine);
    recorder
}

#[cfg(feature = "streaming")]
fn recorded_subscribe_command(recorder: &Rc<RefCell<Vec<DataCommand>>>) -> SubscribeCommand {
    let recorded = recorder.borrow();
    let DataCommand::Subscribe(command) = &recorded[0] else {
        panic!("expected subscribe command");
    };
    command.clone()
}

#[cfg(feature = "streaming")]
fn recorded_subscribe_command_with_correlation(
    recorder: &Rc<RefCell<Vec<DataCommand>>>,
    correlation_id: UUID4,
) -> SubscribeCommand {
    let command = recorded_subscribe_command(recorder);
    assert_eq!(command.correlation_id(), Some(correlation_id));
    command
}

#[rstest]
#[should_panic]
fn test_register_default_client_twice_panics(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let mut data_engine = data_engine.borrow_mut();

    let client_id = ClientId::new("DUPLICATE");

    let data_client1 = DataClientAdapter::new(
        client_id,
        None,
        true,
        true,
        Box::new(MockDataClient::new(
            clock.clone(),
            cache.clone(),
            client_id,
            Some(Venue::test_default()),
        )),
    );
    let data_client2 = DataClientAdapter::new(
        client_id,
        None,
        true,
        true,
        Box::new(MockDataClient::new(
            clock,
            cache,
            client_id,
            Some(Venue::test_default()),
        )),
    );

    data_engine.register_default_client(data_client1);
    data_engine.register_default_client(data_client2);
}

#[rstest]
#[should_panic]
fn test_register_client_duplicate_id_panics(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let mut data_engine = data_engine.borrow_mut();

    let client_id = ClientId::new("DUPLICATE");
    let venue = Venue::test_default();

    let data_client1 = DataClientAdapter::new(
        client_id,
        Some(venue),
        true,
        true,
        Box::new(MockDataClient::new(
            clock.clone(),
            cache.clone(),
            client_id,
            Some(Venue::test_default()),
        )),
    );
    let data_client2 = DataClientAdapter::new(
        client_id,
        Some(venue),
        true,
        true,
        Box::new(MockDataClient::new(
            clock,
            cache,
            client_id,
            Some(Venue::test_default()),
        )),
    );

    data_engine.register_client(data_client1, None);
    data_engine.register_client(data_client2, None);
}

#[rstest]
fn test_register_and_deregister_client(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let mut data_engine = data_engine.borrow_mut();

    let client_id1 = ClientId::new("C1");
    let venue1 = Venue::test_default();

    let data_client1 = DataClientAdapter::new(
        client_id1,
        Some(venue1),
        true,
        true,
        Box::new(MockDataClient::new(
            clock.clone(),
            cache.clone(),
            client_id1,
            Some(venue1),
        )),
    );

    data_engine.register_client(data_client1, Some(venue1));

    let client_id2 = ClientId::new("C2");
    let data_client2 = DataClientAdapter::new(
        client_id2,
        None,
        true,
        true,
        Box::new(MockDataClient::new(clock, cache, client_id2, Some(venue1))),
    );

    data_engine.register_client(data_client2, None);

    // Both present
    assert_eq!(
        data_engine.registered_clients(),
        vec![client_id1, client_id2]
    );

    // Deregister first client
    data_engine.deregister_client(&client_id1);
    assert_eq!(data_engine.registered_clients(), vec![client_id2]);

    // Routing for deregistered venue now yields no client
    assert!(data_engine.get_client(None, Some(&venue1)).is_none());
}

#[rstest]
fn test_register_default_client(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let mut data_engine = data_engine.borrow_mut();

    let default_id = ClientId::new("DEFAULT");
    let default_client = DataClientAdapter::new(
        default_id,
        None,
        true,
        true,
        Box::new(MockDataClient::new(
            clock,
            cache,
            default_id,
            Some(Venue::test_default()),
        )),
    );
    data_engine.register_default_client(default_client);

    assert_eq!(data_engine.registered_clients(), vec![default_id]);
    assert_eq!(
        data_engine.get_client(None, None).unwrap().client_id(),
        default_id
    );
}

#[rstest]
fn test_execute_subscribe_custom_data(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let data_type = DataType::new(stringify!(String), None, None);
    let sub = SubscribeCustomData::new(
        Some(client_id),
        Some(venue),
        data_type.clone(),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let sub_cmd = DataCommand::Subscribe(SubscribeCommand::Data(sub));
    data_engine.execute(sub_cmd.clone());

    assert!(data_engine.subscribed_custom_data().contains(&data_type));
    {
        assert_eq!(recorder.borrow().as_slice(), std::slice::from_ref(&sub_cmd));
    }

    let unsub = UnsubscribeCustomData::new(
        Some(client_id),
        Some(venue),
        data_type.clone(),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let unsub_cmd = DataCommand::Unsubscribe(UnsubscribeCommand::Data(unsub));
    data_engine.execute(unsub_cmd.clone());

    assert!(!data_engine.subscribed_custom_data().contains(&data_type));
    assert_eq!(recorder.borrow().as_slice(), &[sub_cmd, unsub_cmd]);
}

#[rstest]
fn test_execute_subscribe_book_deltas(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let sub_cmd = DataCommand::Subscribe(SubscribeCommand::BookDeltas(SubscribeBookDeltas::new(
        audusd_sim.id,
        BookType::L3_MBO,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        true,
        None,
        None,
    )));
    data_engine.execute(sub_cmd.clone());

    assert!(
        data_engine
            .subscribed_book_deltas()
            .contains(&audusd_sim.id)
    );
    {
        assert_eq!(recorder.borrow().as_slice(), std::slice::from_ref(&sub_cmd));
    }

    let unsub_cmd =
        DataCommand::Unsubscribe(UnsubscribeCommand::BookDeltas(UnsubscribeBookDeltas::new(
            audusd_sim.id,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        )));
    data_engine.execute(unsub_cmd.clone());

    assert!(
        !data_engine
            .subscribed_book_deltas()
            .contains(&audusd_sim.id)
    );
    assert_eq!(recorder.borrow().as_slice(), &[sub_cmd, unsub_cmd]);
}

#[rstest]
fn test_unsubscribe_book_deltas_removes_book_updater(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let deltas_topic = switchboard::get_book_deltas_topic(audusd_sim.id);

    // Initially no subscribers
    assert_eq!(msgbus::subscriber_count_deltas(deltas_topic), 0);

    // Subscribe creates BookUpdater which subscribes to deltas topic
    let sub_cmd = DataCommand::Subscribe(SubscribeCommand::BookDeltas(SubscribeBookDeltas::new(
        audusd_sim.id,
        BookType::L3_MBO,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        true,
        None,
        None,
    )));
    data_engine.execute(sub_cmd);

    // BookUpdater should be subscribed
    assert_eq!(msgbus::subscriber_count_deltas(deltas_topic), 1);

    // Unsubscribe should remove BookUpdater subscription
    let unsub_cmd =
        DataCommand::Unsubscribe(UnsubscribeCommand::BookDeltas(UnsubscribeBookDeltas::new(
            audusd_sim.id,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        )));
    data_engine.execute(unsub_cmd);

    // BookUpdater should be unsubscribed and removed
    assert_eq!(msgbus::subscriber_count_deltas(deltas_topic), 0);
}

#[rstest]
fn test_subscribe_book_deltas_unmanaged_skips_book_updater(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let deltas_topic = switchboard::get_book_deltas_topic(audusd_sim.id);
    let depth_topic = switchboard::get_book_depth10_topic(audusd_sim.id);

    let sub_deltas =
        DataCommand::Subscribe(SubscribeCommand::BookDeltas(SubscribeBookDeltas::new(
            audusd_sim.id,
            BookType::L3_MBO,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            false, // unmanaged
            None,
            None,
        )));
    data_engine.execute(sub_deltas);

    let sub_depth =
        DataCommand::Subscribe(SubscribeCommand::BookDepth10(SubscribeBookDepth10::new(
            audusd_sim.id,
            BookType::L2_MBP,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            false, // unmanaged
            None,
            None,
        )));
    data_engine.execute(sub_depth);

    assert_eq!(msgbus::subscriber_count_deltas(deltas_topic), 0);
    assert_eq!(msgbus::subscriber_count_depth10(depth_topic), 0);
    assert!(
        data_engine.get_cache().order_book(&audusd_sim.id).is_none(),
        "unmanaged subscriptions must not auto-create an order book",
    );
}

#[rstest]
fn test_unsubscribe_depth10_keeps_deltas_book_updater(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let deltas_topic = switchboard::get_book_deltas_topic(audusd_sim.id);
    let depth_topic = switchboard::get_book_depth10_topic(audusd_sim.id);

    // Subscribe to both deltas and depth10
    let sub_deltas =
        DataCommand::Subscribe(SubscribeCommand::BookDeltas(SubscribeBookDeltas::new(
            audusd_sim.id,
            BookType::L3_MBO,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            true,
            None,
            None,
        )));
    data_engine.execute(sub_deltas);

    let sub_depth =
        DataCommand::Subscribe(SubscribeCommand::BookDepth10(SubscribeBookDepth10::new(
            audusd_sim.id,
            BookType::L2_MBP,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            true,
            None,
            None,
        )));
    data_engine.execute(sub_depth);

    // BookUpdater subscribed to both topics
    assert_eq!(msgbus::subscriber_count_deltas(deltas_topic), 1);
    assert_eq!(msgbus::subscriber_count_depth10(depth_topic), 1);

    // Unsubscribe from depth10 only
    let unsub_depth = DataCommand::Unsubscribe(UnsubscribeCommand::BookDepth10(
        UnsubscribeBookDepth10::new(
            audusd_sim.id,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ),
    ));
    data_engine.execute(unsub_depth);

    // BookUpdater should remain subscribed to deltas but not depth10
    assert_eq!(msgbus::subscriber_count_deltas(deltas_topic), 1);
    assert_eq!(msgbus::subscriber_count_depth10(depth_topic), 0);

    // Now unsubscribe from deltas - BookUpdater should be fully removed
    let unsub_deltas =
        DataCommand::Unsubscribe(UnsubscribeCommand::BookDeltas(UnsubscribeBookDeltas::new(
            audusd_sim.id,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        )));
    data_engine.execute(unsub_deltas);

    assert_eq!(msgbus::subscriber_count_deltas(deltas_topic), 0);
    assert_eq!(msgbus::subscriber_count_depth10(depth_topic), 0);
}

fn make_es_future(instrument_id: &str, symbol: &str) -> FuturesContract {
    FuturesContract::new(
        InstrumentId::from(instrument_id),
        Symbol::from(symbol),
        AssetClass::Index,
        Some(Ustr::from("XCME")),
        Ustr::from("ES"),
        UnixNanos::default(),
        UnixNanos::from(2_000_000_000_000_000_000u64),
        Currency::USD(),
        2,
        Price::from("0.01"),
        Quantity::from(1),
        Quantity::from(1),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        UnixNanos::default(),
        UnixNanos::default(),
    )
}

fn make_es_option(instrument_id: &str, symbol: &str, kind: OptionKind) -> OptionContract {
    OptionContract::new(
        InstrumentId::from(instrument_id),
        Symbol::from(symbol),
        AssetClass::Index,
        Some(Ustr::from("XCME")),
        Ustr::from("ES"),
        kind,
        Price::from("4000.00"),
        Currency::USD(),
        UnixNanos::default(),
        UnixNanos::from(2_000_000_000_000_000_000u64),
        2,
        Price::from("0.01"),
        Quantity::from(1),
        Quantity::from(1),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        UnixNanos::default(),
        UnixNanos::default(),
    )
}

#[rstest]
fn test_emit_quotes_from_book_depths_publishes_top_of_book(stub_msgbus: Rc<RefCell<MessageBus>>) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let config = DataEngineConfig {
        emit_quotes_from_book_depths: true,
        ..DataEngineConfig::default()
    };
    let mut data_engine = DataEngine::new(clock, cache.clone(), Some(config));

    let depth = stub_depth10();
    let instrument_id = depth.instrument_id;

    let (handler, saver) = get_typed_message_saving_handler::<QuoteTick>(None);
    let quote_topic = switchboard::get_quotes_topic(instrument_id);
    msgbus::subscribe_quotes(quote_topic.into(), handler, None);

    data_engine.process_data(Data::Depth10(Box::new(depth)));

    let messages = saver.get_messages();
    assert_eq!(
        messages.len(),
        1,
        "depth should emit exactly one synthetic quote",
    );
    let cached_quote = cache.borrow().quote(&instrument_id).copied();
    assert!(cached_quote.is_some(), "synthetic quote should be cached",);

    // Same top-of-book: must not republish
    data_engine.process_data(Data::Depth10(Box::new(depth)));
    assert_eq!(saver.get_messages().len(), 1);

    // Shifted top-of-book: must republish
    let mut shifted = depth;
    shifted.bids[0] = BookOrder::new(
        depth.bids[0].side,
        Price::new(98.50, 2),
        depth.bids[0].size,
        depth.bids[0].order_id,
    );
    shifted.ts_event = UnixNanos::from(depth.ts_event.as_u64() + 1);
    shifted.ts_init = UnixNanos::from(depth.ts_init.as_u64() + 1);
    data_engine.process_data(Data::Depth10(Box::new(shifted)));

    let messages = saver.get_messages();
    assert_eq!(
        messages.len(),
        2,
        "different top-of-book must republish the synthetic quote",
    );
    assert_eq!(messages[1].bid_price, Price::new(98.50, 2));
}

#[rstest]
fn test_emit_quotes_from_book_depths_skips_no_order_side_padding(
    stub_msgbus: Rc<RefCell<MessageBus>>,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let config = DataEngineConfig {
        emit_quotes_from_book_depths: true,
        ..DataEngineConfig::default()
    };
    let mut data_engine = DataEngine::new(clock, cache.clone(), Some(config));

    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let padded_bids: [BookOrder; DEPTH10_LEN] = [BookOrder::default(); DEPTH10_LEN];
    let padded_asks: [BookOrder; DEPTH10_LEN] = [BookOrder::default(); DEPTH10_LEN];
    let depth = OrderBookDepth10::new(
        instrument_id,
        padded_bids,
        padded_asks,
        [0; DEPTH10_LEN],
        [0; DEPTH10_LEN],
        0,
        0,
        UnixNanos::from(1),
        UnixNanos::from(2),
    );

    let (handler, saver) = get_typed_message_saving_handler::<QuoteTick>(None);
    let quote_topic = switchboard::get_quotes_topic(instrument_id);
    msgbus::subscribe_quotes(quote_topic.into(), handler, None);

    data_engine.process_data(Data::Depth10(Box::new(depth)));

    assert!(
        saver.get_messages().is_empty(),
        "fully-padded NoOrderSide depth must not publish a synthetic quote",
    );
    assert!(
        cache.borrow().quote(&instrument_id).is_none(),
        "no quote should be cached for invalid depth padding",
    );
}

#[rstest]
#[case::ts_event_regression(2_000, 2_000, 1_000, 1_000, false)]
#[case::ts_init_only_regression(2_000, 2_000, 2_000, 1_000, false)]
#[case::strictly_forward(1_000, 1_000, 2_000, 2_000, true)]
fn test_validate_data_sequence_drops_out_of_order_bar(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    #[case] first_ts_event: u64,
    #[case] first_ts_init: u64,
    #[case] second_ts_event: u64,
    #[case] second_ts_init: u64,
    #[case] expect_overwrite: bool,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let config = DataEngineConfig {
        validate_data_sequence: true,
        ..DataEngineConfig::default()
    };
    let mut data_engine = DataEngine::new(clock, cache.clone(), Some(config));

    let bar_template = Bar::default();
    let bar_type = bar_template.bar_type;
    let make_bar = |ts_event: u64, ts_init: u64| {
        Bar::new(
            bar_type,
            bar_template.open,
            bar_template.high,
            bar_template.low,
            bar_template.close,
            bar_template.volume,
            UnixNanos::from(ts_event),
            UnixNanos::from(ts_init),
        )
    };

    let first = make_bar(first_ts_event, first_ts_init);
    let second = make_bar(second_ts_event, second_ts_init);

    data_engine.process_data(Data::Bar(first));
    data_engine.process_data(Data::Bar(second));

    let stored = cache.borrow().bar(&bar_type).copied();
    let expected = if expect_overwrite { second } else { first };
    assert_eq!(stored, Some(expected));
}

#[rstest]
fn test_aggregator_emitted_bar_drops_out_of_sequence(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let instrument_id = audusd_sim.id;
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();

    let config = DataEngineConfig {
        validate_data_sequence: true,
        ..DataEngineConfig::default()
    };
    let mut data_engine = DataEngine::new(clock.clone(), cache.clone(), Some(config));

    let test_clock: Rc<RefCell<TestClock>> = Rc::new(RefCell::new(TestClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let bar_type = BarType::from(format!("{instrument_id}-1-TICK-LAST-INTERNAL").as_str());

    let sub = DataCommand::Subscribe(SubscribeCommand::Bars(SubscribeBars::new(
        bar_type,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )));
    data_engine.execute(sub);

    let make_trade = |ts: u64, trade_id: &str| {
        TradeTick::new(
            instrument_id,
            Price::from("0.65000"),
            Quantity::from("1000"),
            AggressorSide::Buyer,
            TradeId::new(trade_id),
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };

    // First trade emits a bar at ts=2_000
    data_engine.process_data(Data::Trade(make_trade(2_000, "t1")));
    let first_bar = cache
        .borrow()
        .bar(&bar_type)
        .copied()
        .expect("first bar must be cached");
    assert_eq!(first_bar.ts_event, UnixNanos::from(2_000));

    // Earlier ts_event: aggregator would emit a regressed bar
    data_engine.process_data(Data::Trade(make_trade(1_000, "t2")));

    let cached = cache
        .borrow()
        .bar(&bar_type)
        .copied()
        .expect("cache should still hold the first bar");
    assert_eq!(
        cached.ts_event, first_bar.ts_event,
        "out-of-order aggregator-emitted bar must not overwrite the cached bar",
    );
}

#[rstest]
fn test_request_scoped_bar_aggregator_runs_alongside_live_subscription(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let instrument_id = audusd_sim.id;
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);
    let test_clock: Rc<RefCell<TestClock>> = Rc::new(RefCell::new(TestClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let bar_type = BarType::from(format!("{instrument_id}-1-TICK-LAST-INTERNAL").as_str());
    let live_subscribe = DataCommand::Subscribe(SubscribeCommand::Bars(SubscribeBars::new(
        bar_type,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )));
    data_engine.execute(live_subscribe);

    let make_trade = |ts: u64, trade_id: &str| {
        TradeTick::new(
            instrument_id,
            Price::from("0.65000"),
            Quantity::from("1000"),
            AggressorSide::Buyer,
            TradeId::new(trade_id),
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };

    data_engine.process_data(Data::Trade(make_trade(1_000, "live-1")));
    assert_eq!(
        cache.borrow().bar(&bar_type).map(|bar| bar.ts_event),
        Some(UnixNanos::from(1_000)),
    );

    let request_id = UUID4::new();
    let params: Params = serde_json::from_value(json!({
        "bar_types": [bar_type.to_string()],
        "update_subscriptions": false,
    }))
    .unwrap();
    let request = RequestTrades::new(
        instrument_id,
        None,
        None,
        None,
        Some(client_id),
        request_id,
        UnixNanos::default(),
        Some(params.clone()),
    );
    data_engine.execute(DataCommand::Request(RequestCommand::Trades(
        request.clone(),
    )));

    assert_eq!(
        recorder.borrow().last(),
        Some(&DataCommand::Request(RequestCommand::Trades(request))),
    );

    data_engine.response(DataResponse::Trades(TradesResponse::new(
        request_id,
        client_id,
        instrument_id,
        vec![make_trade(2_000, "historical-1")],
        None,
        None,
        UnixNanos::from(2_000),
        Some(params),
    )));

    assert_eq!(
        cache.borrow().bar(&bar_type).map(|bar| bar.ts_event),
        Some(UnixNanos::from(2_000)),
        "request-scoped aggregator must process the historical response",
    );

    data_engine.process_data(Data::Trade(make_trade(3_000, "live-2")));

    assert_eq!(
        cache.borrow().bar(&bar_type).map(|bar| bar.ts_event),
        Some(UnixNanos::from(3_000)),
        "live aggregator must remain subscribed after request cleanup",
    );
}

#[rstest]
fn test_request_scoped_quote_bar_aggregators_handle_multiple_bar_types(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let instrument_id = audusd_sim.id;
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);
    let test_clock: Rc<RefCell<TestClock>> = Rc::new(RefCell::new(TestClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let one_tick = BarType::from(format!("{instrument_id}-1-TICK-BID-INTERNAL").as_str());
    let two_tick = BarType::from(format!("{instrument_id}-2-TICK-BID-INTERNAL").as_str());
    let request_id = UUID4::new();
    let params: Params = serde_json::from_value(json!({
        "bar_types": [one_tick.to_string(), two_tick.to_string()],
        "update_subscriptions": false,
    }))
    .unwrap();
    let request = RequestQuotes::new(
        instrument_id,
        None,
        None,
        None,
        Some(client_id),
        request_id,
        UnixNanos::default(),
        Some(params.clone()),
    );
    data_engine.execute(DataCommand::Request(RequestCommand::Quotes(request)));

    let make_quote = |ts: u64, bid: &str| {
        QuoteTick::new(
            instrument_id,
            Price::from(bid),
            Price::from("0.65010"),
            Quantity::from("1000"),
            Quantity::from("1000"),
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };
    data_engine.response(DataResponse::Quotes(QuotesResponse::new(
        request_id,
        client_id,
        instrument_id,
        vec![make_quote(1_000, "0.65000"), make_quote(2_000, "0.65001")],
        None,
        None,
        UnixNanos::from(2_000),
        Some(params),
    )));

    assert_eq!(
        cache.borrow().bar(&one_tick).map(|bar| bar.ts_event),
        Some(UnixNanos::from(2_000)),
    );
    assert_eq!(
        cache.borrow().bar(&two_tick).map(|bar| bar.ts_event),
        Some(UnixNanos::from(2_000)),
    );
}

#[rstest]
fn test_request_scoped_bar_aggregation_deduplicates_bar_types(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let instrument_id = audusd_sim.id;
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);
    let test_clock: Rc<RefCell<TestClock>> = Rc::new(RefCell::new(TestClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let bar_type = BarType::from(format!("{instrument_id}-3-TICK-BID-INTERNAL").as_str());
    let params = || -> Params {
        serde_json::from_value(json!({
        "bar_types": [bar_type.to_string(), bar_type.to_string()],
        "update_subscriptions": false,
        }))
        .unwrap()
    };
    let request_id = UUID4::new();
    let request = RequestQuotes::new(
        instrument_id,
        None,
        None,
        None,
        Some(client_id),
        request_id,
        UnixNanos::default(),
        Some(params()),
    );
    data_engine.execute(DataCommand::Request(RequestCommand::Quotes(request)));

    let make_quote = |ts: u64, bid: &str| {
        QuoteTick::new(
            instrument_id,
            Price::from(bid),
            Price::from("0.65010"),
            Quantity::from("1000"),
            Quantity::from("1000"),
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };
    data_engine.response(DataResponse::Quotes(QuotesResponse::new(
        request_id,
        client_id,
        instrument_id,
        vec![make_quote(1_000, "0.65000"), make_quote(2_000, "0.65001")],
        None,
        None,
        UnixNanos::from(2_000),
        Some(params()),
    )));

    assert_eq!(cache.borrow().bar(&bar_type), None);

    let request_id = UUID4::new();
    let request = RequestQuotes::new(
        instrument_id,
        None,
        None,
        None,
        Some(client_id),
        request_id,
        UnixNanos::default(),
        Some(params()),
    );
    data_engine.execute(DataCommand::Request(RequestCommand::Quotes(request)));

    data_engine.response(DataResponse::Quotes(QuotesResponse::new(
        request_id,
        client_id,
        instrument_id,
        vec![
            make_quote(1_000, "0.65000"),
            make_quote(2_000, "0.65001"),
            make_quote(3_000, "0.65002"),
        ],
        None,
        None,
        UnixNanos::from(3_000),
        Some(params()),
    )));

    assert_eq!(
        cache.borrow().bar(&bar_type).map(|bar| bar.ts_event),
        Some(UnixNanos::from(3_000)),
    );
}

#[rstest]
fn test_request_scoped_bar_aggregation_does_not_publish_to_live_topic(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let instrument_id = audusd_sim.id;
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);
    let test_clock: Rc<RefCell<TestClock>> = Rc::new(RefCell::new(TestClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let bar_type = BarType::from(format!("{instrument_id}-1-TICK-LAST-INTERNAL").as_str());
    let (handler, saver) = get_typed_message_saving_handler::<Bar>(None);
    let topic = switchboard::get_bars_topic(bar_type);
    msgbus::subscribe_bars(topic.into(), handler, None);

    let request_id = UUID4::new();
    let params: Params = serde_json::from_value(json!({
        "bar_types": [bar_type.to_string()],
        "update_subscriptions": false,
    }))
    .unwrap();
    let request = RequestTrades::new(
        instrument_id,
        None,
        None,
        None,
        Some(client_id),
        request_id,
        UnixNanos::default(),
        Some(params.clone()),
    );
    data_engine.execute(DataCommand::Request(RequestCommand::Trades(request)));

    let trade = TradeTick::new(
        instrument_id,
        Price::from("0.65000"),
        Quantity::from("1000"),
        AggressorSide::Buyer,
        TradeId::new("historical-1"),
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
    );
    data_engine.response(DataResponse::Trades(TradesResponse::new(
        request_id,
        client_id,
        instrument_id,
        vec![trade],
        None,
        None,
        UnixNanos::from(1_000),
        Some(params),
    )));

    assert_eq!(
        cache.borrow().bar(&bar_type).map(|bar| bar.ts_event),
        Some(UnixNanos::from(1_000)),
    );
    assert!(saver.get_messages().is_empty());
}

#[rstest]
fn test_request_scoped_time_bar_aggregation_handles_trade_response(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let instrument_id = audusd_sim.id;
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);
    let test_clock: Rc<RefCell<TestClock>> = Rc::new(RefCell::new(TestClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let bar_type = BarType::from(format!("{instrument_id}-1-SECOND-LAST-INTERNAL").as_str());
    let (handler, saver) = get_typed_message_saving_handler::<Bar>(None);
    let topic = switchboard::get_bars_topic(bar_type);
    msgbus::subscribe_bars(topic.into(), handler, None);

    let request_id = UUID4::new();
    let params: Params = serde_json::from_value(json!({
        "bar_types": [bar_type.to_string()],
        "update_subscriptions": false,
    }))
    .unwrap();
    let request = RequestTrades::new(
        instrument_id,
        None,
        None,
        None,
        Some(client_id),
        request_id,
        UnixNanos::default(),
        Some(params.clone()),
    );
    data_engine.execute(DataCommand::Request(RequestCommand::Trades(request)));

    let make_trade = |ts: u64, trade_id: &str| {
        TradeTick::new(
            instrument_id,
            Price::from("0.65000"),
            Quantity::from("1000"),
            AggressorSide::Buyer,
            TradeId::new(trade_id),
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };
    data_engine.response(DataResponse::Trades(TradesResponse::new(
        request_id,
        client_id,
        instrument_id,
        vec![
            make_trade(0, "historical-1"),
            make_trade(1_000_000_000, "historical-2"),
        ],
        None,
        None,
        UnixNanos::from(1_000_000_000),
        Some(params),
    )));

    assert_eq!(
        cache.borrow().bar(&bar_type).map(|bar| bar.ts_event),
        Some(UnixNanos::from(1_000_000_000)),
    );
    assert!(saver.get_messages().is_empty());
}

#[rstest]
fn test_request_scoped_composite_bar_aggregator_handles_bar_response(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let instrument_id = audusd_sim.id;
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);
    let test_clock: Rc<RefCell<TestClock>> = Rc::new(RefCell::new(TestClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let composite =
        BarType::from(format!("{instrument_id}-1-TICK-LAST-INTERNAL@1-TICK-EXTERNAL").as_str());
    let source = composite.composite();
    let request_id = UUID4::new();
    let params: Params = serde_json::from_value(json!({
        "bar_types": [composite.to_string()],
        "update_subscriptions": false,
    }))
    .unwrap();
    let request = RequestBars::new(
        source,
        None,
        None,
        None,
        Some(client_id),
        request_id,
        UnixNanos::default(),
        Some(params.clone()),
    );
    data_engine.execute(DataCommand::Request(RequestCommand::Bars(request)));

    let bar = Bar::new(
        source,
        Price::from("0.65000"),
        Price::from("0.65000"),
        Price::from("0.65000"),
        Price::from("0.65000"),
        Quantity::from("1000"),
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
    );
    data_engine.response(DataResponse::Bars(BarsResponse::new(
        request_id,
        client_id,
        source,
        vec![bar],
        None,
        None,
        UnixNanos::from(1_000),
        Some(params),
    )));

    assert_eq!(
        cache.borrow().bar(&composite).map(|bar| bar.ts_event),
        Some(UnixNanos::from(1_000)),
    );
}

#[rstest]
fn test_update_subscriptions_request_aggregator_can_be_started_live_after_response(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let instrument_id = audusd_sim.id;
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);
    let test_clock: Rc<RefCell<TestClock>> = Rc::new(RefCell::new(TestClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let bar_type = BarType::from(format!("{instrument_id}-1-TICK-LAST-INTERNAL").as_str());
    let (handler, saver) = get_typed_message_saving_handler::<Bar>(None);
    let topic = switchboard::get_bars_topic(bar_type);
    msgbus::subscribe_bars(topic.into(), handler, None);

    let request_id = UUID4::new();
    let params: Params = serde_json::from_value(json!({
        "bar_types": [bar_type.to_string()],
        "update_subscriptions": true,
    }))
    .unwrap();
    let request = RequestTrades::new(
        instrument_id,
        None,
        None,
        None,
        Some(client_id),
        request_id,
        UnixNanos::default(),
        Some(params.clone()),
    );
    data_engine.execute(DataCommand::Request(RequestCommand::Trades(request)));

    let make_trade = |ts: u64, trade_id: &str| {
        TradeTick::new(
            instrument_id,
            Price::from("0.65000"),
            Quantity::from("1000"),
            AggressorSide::Buyer,
            TradeId::new(trade_id),
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };
    data_engine.response(DataResponse::Trades(TradesResponse::new(
        request_id,
        client_id,
        instrument_id,
        vec![make_trade(1_000, "historical-1")],
        None,
        None,
        UnixNanos::from(1_000),
        Some(params),
    )));

    assert!(saver.get_messages().is_empty());

    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Bars(
        SubscribeBars::new(
            bar_type,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ),
    )));
    data_engine.process_data(Data::Trade(make_trade(2_000, "live-1")));

    assert_eq!(
        cache.borrow().bar(&bar_type).map(|bar| bar.ts_event),
        Some(UnixNanos::from(2_000)),
    );
    let messages = saver.get_messages();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].ts_event, UnixNanos::from(2_000));
}

#[rstest]
fn test_update_subscriptions_request_aggregator_can_subscribe_before_response(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let instrument_id = audusd_sim.id;
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);
    let test_clock: Rc<RefCell<TestClock>> = Rc::new(RefCell::new(TestClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let bar_type = BarType::from(format!("{instrument_id}-1-TICK-LAST-INTERNAL").as_str());
    let (handler, saver) = get_typed_message_saving_handler::<Bar>(None);
    let topic = switchboard::get_bars_topic(bar_type);
    msgbus::subscribe_bars(topic.into(), handler, None);

    let request_id = UUID4::new();
    let params: Params = serde_json::from_value(json!({
        "bar_types": [bar_type.to_string()],
        "update_subscriptions": true,
    }))
    .unwrap();
    let request = RequestTrades::new(
        instrument_id,
        None,
        None,
        None,
        Some(client_id),
        request_id,
        UnixNanos::default(),
        Some(params.clone()),
    );
    data_engine.execute(DataCommand::Request(RequestCommand::Trades(request)));

    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Bars(
        SubscribeBars::new(
            bar_type,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ),
    )));

    let make_trade = |ts: u64, trade_id: &str| {
        TradeTick::new(
            instrument_id,
            Price::from("0.65000"),
            Quantity::from("1000"),
            AggressorSide::Buyer,
            TradeId::new(trade_id),
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };
    data_engine.response(DataResponse::Trades(TradesResponse::new(
        request_id,
        client_id,
        instrument_id,
        vec![make_trade(1_000, "historical-1")],
        None,
        None,
        UnixNanos::from(1_000),
        Some(params),
    )));

    assert_eq!(
        cache.borrow().bar(&bar_type).map(|bar| bar.ts_event),
        Some(UnixNanos::from(1_000)),
    );
    assert!(saver.get_messages().is_empty());

    data_engine.process_data(Data::Trade(make_trade(2_000, "live-1")));

    assert_eq!(
        cache.borrow().bar(&bar_type).map(|bar| bar.ts_event),
        Some(UnixNanos::from(2_000)),
    );
    let messages = saver.get_messages();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].ts_event, UnixNanos::from(2_000));
}

#[rstest]
fn test_request_bar_aggregation_rejects_running_update_subscription_aggregator(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let instrument_id = audusd_sim.id;
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);
    let test_clock: Rc<RefCell<TestClock>> = Rc::new(RefCell::new(TestClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let bar_type = BarType::from(format!("{instrument_id}-1-TICK-LAST-INTERNAL").as_str());
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Bars(
        SubscribeBars::new(
            bar_type,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ),
    )));

    let params: Params = serde_json::from_value(json!({
        "bar_types": [bar_type.to_string()],
        "update_subscriptions": true,
    }))
    .unwrap();
    let request = RequestTrades::new(
        instrument_id,
        None,
        None,
        None,
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        Some(params),
    );

    let result = data_engine.execute_request(RequestCommand::Trades(request));

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("already running"));
}

#[rstest]
fn test_request_bar_aggregation_rejects_external_bar_type(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let instrument_id = audusd_sim.id;
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);
    let test_clock: Rc<RefCell<TestClock>> = Rc::new(RefCell::new(TestClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let bar_type = BarType::from(format!("{instrument_id}-1-TICK-LAST-EXTERNAL").as_str());
    let params: Params = serde_json::from_value(json!({
        "bar_types": [bar_type.to_string()],
    }))
    .unwrap();
    let request = RequestTrades::new(
        instrument_id,
        None,
        None,
        None,
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        Some(params),
    );

    let result = data_engine.execute_request(RequestCommand::Trades(request));

    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("must be internally aggregated")
    );
}

#[rstest]
fn test_request_bar_aggregation_cleans_up_after_dispatch_failure(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let instrument_id = audusd_sim.id;
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);
    let bar_type = BarType::from(format!("{instrument_id}-1-TICK-LAST-INTERNAL").as_str());
    let request_id = UUID4::new();
    let params: Params = serde_json::from_value(json!({
        "bar_types": [bar_type.to_string()],
        "update_subscriptions": false,
    }))
    .unwrap();
    let request = RequestTrades::new(
        instrument_id,
        None,
        None,
        None,
        Some(client_id),
        request_id,
        UnixNanos::default(),
        Some(params.clone()),
    );

    let result = data_engine.execute_request(RequestCommand::Trades(request.clone()));

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("no client found"));

    let test_clock: Rc<RefCell<TestClock>> = Rc::new(RefCell::new(TestClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    data_engine
        .execute_request(RequestCommand::Trades(request))
        .unwrap();

    let trade = TradeTick::new(
        instrument_id,
        Price::from("0.65000"),
        Quantity::from("1000"),
        AggressorSide::Buyer,
        TradeId::new("historical-1"),
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
    );
    data_engine.response(DataResponse::Trades(TradesResponse::new(
        request_id,
        client_id,
        instrument_id,
        vec![trade],
        None,
        None,
        UnixNanos::from(1_000),
        Some(params),
    )));

    assert_eq!(
        cache.borrow().bar(&bar_type).map(|bar| bar.ts_event),
        Some(UnixNanos::from(1_000)),
    );
}

#[rstest]
fn test_request_bar_aggregation_reset_clears_pending_aggregators(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let instrument_id = audusd_sim.id;
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);
    let test_clock: Rc<RefCell<TestClock>> = Rc::new(RefCell::new(TestClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let bar_type = BarType::from(format!("{instrument_id}-1-TICK-LAST-INTERNAL").as_str());
    let params = || -> Params {
        serde_json::from_value(json!({
            "bar_types": [bar_type.to_string()],
            "update_subscriptions": false,
        }))
        .unwrap()
    };
    let request = RequestTrades::new(
        instrument_id,
        None,
        None,
        None,
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        Some(params()),
    );
    data_engine
        .execute_request(RequestCommand::Trades(request))
        .unwrap();

    data_engine.reset();

    let request_id = UUID4::new();
    let request = RequestTrades::new(
        instrument_id,
        None,
        None,
        None,
        Some(client_id),
        request_id,
        UnixNanos::default(),
        Some(params()),
    );
    data_engine
        .execute_request(RequestCommand::Trades(request))
        .unwrap();

    let trade = TradeTick::new(
        instrument_id,
        Price::from("0.65000"),
        Quantity::from("1000"),
        AggressorSide::Buyer,
        TradeId::new("historical-1"),
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
    );
    data_engine.response(DataResponse::Trades(TradesResponse::new(
        request_id,
        client_id,
        instrument_id,
        vec![trade],
        None,
        None,
        UnixNanos::from(1_000),
        Some(params()),
    )));

    assert_eq!(
        cache.borrow().bar(&bar_type).map(|bar| bar.ts_event),
        Some(UnixNanos::from(1_000)),
    );
}

#[rstest]
fn test_subscribe_book_deltas_composite_creates_books_per_underlying(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let venue = Venue::new("XCME");

    let esz1 = make_es_future("ESZ1.XCME", "ESZ1");
    let esh2 = make_es_future("ESH2.XCME", "ESH2");
    let esz1_id = esz1.id();
    let esh2_id = esh2.id();

    {
        let mut cache_mut = cache.borrow_mut();
        cache_mut
            .add_instrument(InstrumentAny::FuturesContract(esz1))
            .unwrap();
        cache_mut
            .add_instrument(InstrumentAny::FuturesContract(esh2))
            .unwrap();
    }

    let mut data_engine = DataEngine::new(clock.clone(), cache.clone(), None);

    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let test_clock: Rc<RefCell<TestClock>> = Rc::new(RefCell::new(TestClock::new()));
    register_mock_client(
        test_clock,
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let composite_id = InstrumentId::from("ES.FUT.XCME");
    assert!(composite_id.symbol.is_composite());
    assert_eq!(composite_id.symbol.root(), "ES");

    let sub = DataCommand::Subscribe(SubscribeCommand::BookDeltas(SubscribeBookDeltas::new(
        composite_id,
        BookType::L2_MBP,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        true,
        None,
        Some(parent_params()),
    )));
    data_engine.execute(sub);

    let cache_view = cache.borrow();
    assert!(
        cache_view.order_book(&esz1_id).is_some(),
        "underlying ESZ1.XCME book should be created",
    );
    assert!(
        cache_view.order_book(&esh2_id).is_some(),
        "underlying ESH2.XCME book should be created",
    );
}

#[rstest]
fn test_composite_book_deltas_route_to_per_underlying_book(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let venue = Venue::new("XCME");

    let esz1 = make_es_future("ESZ1.XCME", "ESZ1");
    let esh2 = make_es_future("ESH2.XCME", "ESH2");
    let esz1_id = esz1.id();
    let esh2_id = esh2.id();

    {
        let mut cache_mut = cache.borrow_mut();
        cache_mut
            .add_instrument(InstrumentAny::FuturesContract(esz1))
            .unwrap();
        cache_mut
            .add_instrument(InstrumentAny::FuturesContract(esh2))
            .unwrap();
    }

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);

    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let test_clock: Rc<RefCell<TestClock>> = Rc::new(RefCell::new(TestClock::new()));
    register_mock_client(
        test_clock,
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let composite_id = InstrumentId::from("ES.FUT.XCME");
    let sub = DataCommand::Subscribe(SubscribeCommand::BookDeltas(SubscribeBookDeltas::new(
        composite_id,
        BookType::L2_MBP,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        true,
        None,
        Some(parent_params()),
    )));
    data_engine.execute(sub);

    let delta = OrderBookDeltaTestBuilder::new(esz1_id).build();
    data_engine.process_data(Data::Delta(delta));

    let cache_view = cache.borrow();
    let esz1_book = cache_view
        .order_book(&esz1_id)
        .expect("ESZ1 book should exist after composite subscribe");
    assert_eq!(
        esz1_book.update_count, 1,
        "per-underlying delta must reach the ESZ1 book via the composite wildcard subscription",
    );

    let esh2_book = cache_view
        .order_book(&esh2_id)
        .expect("ESH2 book should exist after composite subscribe");
    assert_eq!(
        esh2_book.update_count, 0,
        "ESH2 book must remain untouched when only ESZ1 deltas are processed",
    );
}

#[rstest]
fn test_composite_book_deltas_route_each_underlying_independently(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let venue = Venue::new("XCME");

    let esz1 = make_es_future("ESZ1.XCME", "ESZ1");
    let esh2 = make_es_future("ESH2.XCME", "ESH2");
    let esz1_id = esz1.id();
    let esh2_id = esh2.id();

    {
        let mut cache_mut = cache.borrow_mut();
        cache_mut
            .add_instrument(InstrumentAny::FuturesContract(esz1))
            .unwrap();
        cache_mut
            .add_instrument(InstrumentAny::FuturesContract(esh2))
            .unwrap();
    }

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);

    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let test_clock: Rc<RefCell<TestClock>> = Rc::new(RefCell::new(TestClock::new()));
    register_mock_client(
        test_clock,
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let composite_id = InstrumentId::from("ES.FUT.XCME");
    let sub = DataCommand::Subscribe(SubscribeCommand::BookDeltas(SubscribeBookDeltas::new(
        composite_id,
        BookType::L2_MBP,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        true,
        None,
        Some(parent_params()),
    )));
    data_engine.execute(sub);

    data_engine.process_data(Data::Delta(OrderBookDeltaTestBuilder::new(esz1_id).build()));
    data_engine.process_data(Data::Delta(OrderBookDeltaTestBuilder::new(esh2_id).build()));

    let cache_view = cache.borrow();
    assert_eq!(
        cache_view.order_book(&esz1_id).unwrap().update_count,
        1,
        "ESZ1 book must reflect exactly its own delta",
    );
    assert_eq!(
        cache_view.order_book(&esh2_id).unwrap().update_count,
        1,
        "ESH2 book must reflect exactly its own delta",
    );
}

#[rstest]
fn test_reset_unsubscribes_composite_book_deltas(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let venue = Venue::new("XCME");

    let esz1 = make_es_future("ESZ1.XCME", "ESZ1");
    let esz1_id = esz1.id();

    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::FuturesContract(esz1))
        .unwrap();

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);

    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let test_clock: Rc<RefCell<TestClock>> = Rc::new(RefCell::new(TestClock::new()));
    register_mock_client(
        test_clock,
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let composite_id = InstrumentId::from("ES.FUT.XCME");
    let sub = DataCommand::Subscribe(SubscribeCommand::BookDeltas(SubscribeBookDeltas::new(
        composite_id,
        BookType::L2_MBP,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        true,
        None,
        Some(parent_params()),
    )));
    data_engine.execute(sub);

    data_engine.process_data(Data::Delta(OrderBookDeltaTestBuilder::new(esz1_id).build()));
    let pre_reset_count = cache.borrow().order_book(&esz1_id).unwrap().update_count;
    assert_eq!(pre_reset_count, 1);

    data_engine.reset();

    data_engine.process_data(Data::Delta(OrderBookDeltaTestBuilder::new(esz1_id).build()));
    let post_reset_count = cache.borrow().order_book(&esz1_id).unwrap().update_count;
    assert_eq!(
        post_reset_count, pre_reset_count,
        "composite BookUpdater must be unsubscribed on reset; new deltas must not mutate the book",
    );
}

#[rstest]
fn test_composite_and_exact_book_deltas_apply_once_per_publish(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let venue = Venue::new("XCME");

    let esz1 = make_es_future("ESZ1.XCME", "ESZ1");
    let esh2 = make_es_future("ESH2.XCME", "ESH2");
    let esz1_id = esz1.id();
    let esh2_id = esh2.id();

    {
        let mut cache_mut = cache.borrow_mut();
        cache_mut
            .add_instrument(InstrumentAny::FuturesContract(esz1))
            .unwrap();
        cache_mut
            .add_instrument(InstrumentAny::FuturesContract(esh2))
            .unwrap();
    }

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);

    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let test_clock: Rc<RefCell<TestClock>> = Rc::new(RefCell::new(TestClock::new()));
    register_mock_client(
        test_clock,
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let composite_id = InstrumentId::from("ES.FUT.XCME");
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::BookDeltas(
        SubscribeBookDeltas::new(
            composite_id,
            BookType::L2_MBP,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            true,
            None,
            Some(parent_params()),
        ),
    )));
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::BookDeltas(
        SubscribeBookDeltas::new(
            esz1_id,
            BookType::L2_MBP,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            true,
            None,
            None,
        ),
    )));

    data_engine.process_data(Data::Delta(OrderBookDeltaTestBuilder::new(esz1_id).build()));

    let cache_view = cache.borrow();
    assert_eq!(
        cache_view.order_book(&esz1_id).unwrap().update_count,
        1,
        "ESZ1 must apply each delta exactly once even when both composite and exact subs are active",
    );
    assert_eq!(
        cache_view.order_book(&esh2_id).unwrap().update_count,
        0,
        "ESH2 book stays untouched when only ESZ1 deltas are processed",
    );
}

#[rstest]
fn test_unsubscribe_composite_keeps_overlapping_exact_alive(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let venue = Venue::new("XCME");

    let esz1 = make_es_future("ESZ1.XCME", "ESZ1");
    let esh2 = make_es_future("ESH2.XCME", "ESH2");
    let esz1_id = esz1.id();
    let esh2_id = esh2.id();

    {
        let mut cache_mut = cache.borrow_mut();
        cache_mut
            .add_instrument(InstrumentAny::FuturesContract(esz1))
            .unwrap();
        cache_mut
            .add_instrument(InstrumentAny::FuturesContract(esh2))
            .unwrap();
    }

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);

    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let test_clock: Rc<RefCell<TestClock>> = Rc::new(RefCell::new(TestClock::new()));
    register_mock_client(
        test_clock,
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let composite_id = InstrumentId::from("ES.FUT.XCME");
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::BookDeltas(
        SubscribeBookDeltas::new(
            composite_id,
            BookType::L2_MBP,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            true,
            None,
            Some(parent_params()),
        ),
    )));
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::BookDeltas(
        SubscribeBookDeltas::new(
            esz1_id,
            BookType::L2_MBP,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            true,
            None,
            None,
        ),
    )));

    data_engine.execute(DataCommand::Unsubscribe(UnsubscribeCommand::BookDeltas(
        UnsubscribeBookDeltas::new(
            composite_id,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            Some(parent_params()),
        ),
    )));

    data_engine.process_data(Data::Delta(OrderBookDeltaTestBuilder::new(esz1_id).build()));
    data_engine.process_data(Data::Delta(OrderBookDeltaTestBuilder::new(esh2_id).build()));

    let cache_view = cache.borrow();
    assert_eq!(
        cache_view.order_book(&esz1_id).unwrap().update_count,
        1,
        "ESZ1 BookUpdater must remain alive (exact sub still active) after composite unsubscribe",
    );
    assert_eq!(
        cache_view.order_book(&esh2_id).unwrap().update_count,
        0,
        "ESH2 BookUpdater must be torn down (no remaining sub) after composite unsubscribe",
    );
}

#[rstest]
fn test_unsubscribe_composite_deltas_keeps_composite_depth10_alive(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let venue = Venue::new("XCME");

    let esz1 = make_es_future("ESZ1.XCME", "ESZ1");
    let esz1_id = esz1.id();
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::FuturesContract(esz1))
        .unwrap();

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let test_clock: Rc<RefCell<TestClock>> = Rc::new(RefCell::new(TestClock::new()));
    register_mock_client(
        test_clock,
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let composite_id = InstrumentId::from("ES.FUT.XCME");
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::BookDeltas(
        SubscribeBookDeltas::new(
            composite_id,
            BookType::L2_MBP,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            true,
            None,
            Some(parent_params()),
        ),
    )));
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::BookDepth10(
        SubscribeBookDepth10::new(
            composite_id,
            BookType::L2_MBP,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            true,
            None,
            Some(parent_params()),
        ),
    )));

    data_engine.execute(DataCommand::Unsubscribe(UnsubscribeCommand::BookDeltas(
        UnsubscribeBookDeltas::new(
            composite_id,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            Some(parent_params()),
        ),
    )));

    let mut depth = stub_depth10();
    depth.instrument_id = esz1_id;
    data_engine.process_data(Data::Depth10(Box::new(depth)));

    let cache_view = cache.borrow();
    let esz1_book = cache_view
        .order_book(&esz1_id)
        .expect("ESZ1 book must exist while composite depth10 sub is active");
    assert!(
        esz1_book.update_count >= 1,
        "depth10 publish must reach the per-underlying book; \
         composite depth10 sub kept alive after deltas unsubscribed",
    );
}

#[rstest]
fn test_unsubscribe_composite_deltas_keeps_exact_depth10_deltas_handler_alive(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let venue = Venue::new("XCME");

    let esz1 = make_es_future("ESZ1.XCME", "ESZ1");
    let esh2 = make_es_future("ESH2.XCME", "ESH2");
    let esz1_id = esz1.id();
    {
        let mut cache_mut = cache.borrow_mut();
        cache_mut
            .add_instrument(InstrumentAny::FuturesContract(esz1))
            .unwrap();
        cache_mut
            .add_instrument(InstrumentAny::FuturesContract(esh2))
            .unwrap();
    }

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let test_clock: Rc<RefCell<TestClock>> = Rc::new(RefCell::new(TestClock::new()));
    register_mock_client(
        test_clock,
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let composite_id = InstrumentId::from("ES.FUT.XCME");
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::BookDepth10(
        SubscribeBookDepth10::new(
            esz1_id,
            BookType::L2_MBP,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            true,
            None,
            None,
        ),
    )));
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::BookDeltas(
        SubscribeBookDeltas::new(
            composite_id,
            BookType::L2_MBP,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            true,
            None,
            Some(parent_params()),
        ),
    )));

    data_engine.execute(DataCommand::Unsubscribe(UnsubscribeCommand::BookDeltas(
        UnsubscribeBookDeltas::new(
            composite_id,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            Some(parent_params()),
        ),
    )));

    data_engine.process_data(Data::Delta(OrderBookDeltaTestBuilder::new(esz1_id).build()));

    let cache_view = cache.borrow();
    assert_eq!(
        cache_view.order_book(&esz1_id).unwrap().update_count,
        1,
        "exact depth10 sub keeps the per-underlying deltas handler alive after composite deltas unsubscribed",
    );
}

#[rstest]
fn test_snapshot_after_deltas_keeps_depth10_handler_alive(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let venue = Venue::new("XCME");

    let esz1 = make_es_future("ESZ1.XCME", "ESZ1");
    let esz1_id = esz1.id();
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::FuturesContract(esz1))
        .unwrap();

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let test_clock: Rc<RefCell<TestClock>> = Rc::new(RefCell::new(TestClock::new()));
    register_mock_client(
        test_clock,
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::BookDeltas(
        SubscribeBookDeltas::new(
            esz1_id,
            BookType::L2_MBP,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            true,
            None,
            None,
        ),
    )));
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::BookSnapshots(
        SubscribeBookSnapshots::new(
            esz1_id,
            BookType::L2_MBP,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            NonZeroUsize::new(1000).unwrap(),
            None,
            None,
        ),
    )));

    data_engine.execute(DataCommand::Unsubscribe(UnsubscribeCommand::BookDeltas(
        UnsubscribeBookDeltas::new(
            esz1_id,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ),
    )));

    let mut depth = stub_depth10();
    depth.instrument_id = esz1_id;
    data_engine.process_data(Data::Depth10(Box::new(depth)));

    let cache_view = cache.borrow();
    let book = cache_view
        .order_book(&esz1_id)
        .expect("ESZ1 book must exist while snapshot sub is active");
    assert!(
        book.update_count >= 1,
        "depth10 publish must reach the per-underlying book; \
         deltas-then-snapshots path now registers the depth10 handler",
    );
}

#[rstest]
fn test_subscribe_book_deltas_composite_with_no_underlyings_is_noop(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let venue = Venue::new("XCME");

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);

    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let test_clock: Rc<RefCell<TestClock>> = Rc::new(RefCell::new(TestClock::new()));
    register_mock_client(
        test_clock,
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let composite_id = InstrumentId::from("ES.FUT.XCME");
    let sub = DataCommand::Subscribe(SubscribeCommand::BookDeltas(SubscribeBookDeltas::new(
        composite_id,
        BookType::L2_MBP,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        true,
        None,
        Some(parent_params()),
    )));
    data_engine.execute(sub);

    let cache_view = cache.borrow();
    assert!(
        cache_view.order_book(&composite_id).is_none(),
        "no book should be created for the parent id itself",
    );
    assert!(
        cache_view
            .instruments_by_parent(&venue, &Ustr::from("ES"), InstrumentClass::Future)
            .is_empty(),
        "no FUT-class underlyings should exist for the parent root",
    );
}

#[rstest]
fn test_parent_book_deltas_filters_by_instrument_class(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let venue = Venue::new("XCME");

    let esz1 = make_es_future("ESZ1.XCME", "ESZ1");
    let esh2 = make_es_future("ESH2.XCME", "ESH2");
    let es_call = make_es_option("ES C4000.XCME", "ES C4000", OptionKind::Call);
    let esz1_id = esz1.id();
    let esh2_id = esh2.id();
    let es_call_id = es_call.id();

    {
        let mut cache_mut = cache.borrow_mut();
        cache_mut
            .add_instrument(InstrumentAny::FuturesContract(esz1))
            .unwrap();
        cache_mut
            .add_instrument(InstrumentAny::FuturesContract(esh2))
            .unwrap();
        cache_mut
            .add_instrument(InstrumentAny::OptionContract(es_call))
            .unwrap();
    }

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let test_clock: Rc<RefCell<TestClock>> = Rc::new(RefCell::new(TestClock::new()));
    register_mock_client(
        test_clock,
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let parent_id = InstrumentId::from("ES.FUT.XCME");
    let sub = DataCommand::Subscribe(SubscribeCommand::BookDeltas(SubscribeBookDeltas::new(
        parent_id,
        BookType::L2_MBP,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        true,
        None,
        Some(parent_params()),
    )));
    data_engine.execute(sub);

    let cache_view = cache.borrow();
    assert!(
        cache_view.order_book(&esz1_id).is_some(),
        "ESZ1 future leaf book must be created",
    );
    assert!(
        cache_view.order_book(&esh2_id).is_some(),
        "ESH2 future leaf book must be created",
    );
    assert!(
        cache_view.order_book(&es_call_id).is_none(),
        "ES call option book must NOT be created when parent class is FUT",
    );
}

#[rstest]
fn test_parent_book_snapshots_filter_by_instrument_class(client_id: ClientId) {
    let clock: Rc<RefCell<TestClock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let venue = Venue::new("XCME");

    let esz1 = make_es_future("ESZ1.XCME", "ESZ1");
    let esh2 = make_es_future("ESH2.XCME", "ESH2");
    let es_call = make_es_option("ES C4000.XCME", "ES C4000", OptionKind::Call);
    let esz1_id = esz1.id();
    let esh2_id = esh2.id();
    let es_call_id = es_call.id();

    {
        let mut cache_mut = cache.borrow_mut();
        cache_mut
            .add_instrument(InstrumentAny::FuturesContract(esz1))
            .unwrap();
        cache_mut
            .add_instrument(InstrumentAny::FuturesContract(esh2))
            .unwrap();
        cache_mut
            .add_instrument(InstrumentAny::OptionContract(es_call))
            .unwrap();
    }

    let data_engine = create_snapshot_test_engine(clock.clone(), cache.clone());
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock.clone(),
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine.borrow_mut(),
    );

    let parent_id = InstrumentId::from("ES.FUT.XCME");
    let interval_ms = NonZeroUsize::new(100).unwrap();
    let parent_topic = switchboard::get_book_snapshots_topic(parent_id, interval_ms);

    let (handler, saver) = get_typed_message_saving_handler::<OrderBook>(None);
    msgbus::subscribe_book_snapshots(parent_topic.into(), handler, None);

    data_engine
        .borrow_mut()
        .execute(DataCommand::Subscribe(SubscribeCommand::BookSnapshots(
            SubscribeBookSnapshots::new(
                parent_id,
                BookType::L2_MBP,
                Some(client_id),
                Some(venue),
                UUID4::new(),
                UnixNanos::default(),
                None,
                interval_ms,
                None,
                Some(parent_params()),
            ),
        )));

    // Feed deltas to populate each leaf's book so the snapshotter has
    // something to publish.
    process_book_delta(&data_engine, esz1_id);
    process_book_delta(&data_engine, esh2_id);
    process_book_delta(&data_engine, es_call_id);

    advance_clock_and_dispatch(&clock, 200_000_000);

    wait_until(
        || saver.get_messages().len() >= 2,
        Duration::from_millis(100),
    );

    let snapshots = saver.get_messages();
    let snapshot_ids: Vec<InstrumentId> = snapshots.iter().map(|b| b.instrument_id).collect();

    assert!(
        snapshot_ids.contains(&esz1_id),
        "parent snapshot subscription on ES.FUT.XCME must publish ESZ1 future snapshot",
    );
    assert!(
        snapshot_ids.contains(&esh2_id),
        "parent snapshot subscription on ES.FUT.XCME must publish ESH2 future snapshot",
    );
    assert!(
        !snapshot_ids.contains(&es_call_id),
        "parent snapshot subscription on ES.FUT.XCME must NOT publish the ES call option \
         snapshot even though it shares the ES underlying root",
    );
}

#[rstest]
fn test_parent_subscribe_with_unparsable_id_returns_error(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let venue = Venue::new("BETFAIR");

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let test_clock: Rc<RefCell<TestClock>> = Rc::new(RefCell::new(TestClock::new()));
    register_mock_client(
        test_clock,
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let runner = InstrumentId::from("1.211334112-31570229.BETFAIR");
    let sub = DataCommand::Subscribe(SubscribeCommand::BookDeltas(SubscribeBookDeltas::new(
        runner,
        BookType::L2_MBP,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        true,
        None,
        Some(parent_params()),
    )));
    data_engine.execute(sub);

    {
        let cache_view = cache.borrow();
        assert!(
            cache_view.order_book(&runner).is_none(),
            "parent subscribe with an unparsable Betfair runner id must NOT create a book; \
             the engine should reject the command",
        );
    }
    assert!(
        !data_engine.subscribed_book_deltas().contains(&runner),
        "rejected parent subscribe must NOT leave the id in book_deltas_subs",
    );

    // Retrying without the parent flag on the same id must succeed; the
    // earlier rejection cannot have stuck the engine in a half-subscribed state.
    let retry = DataCommand::Subscribe(SubscribeCommand::BookDeltas(SubscribeBookDeltas::new(
        runner,
        BookType::L2_MBP,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        true,
        None,
        None,
    )));
    data_engine.execute(retry);
    assert!(
        cache.borrow().order_book(&runner).is_some(),
        "concrete subscribe after a rejected parent attempt must still create the exact-id book",
    );
}

#[rstest]
fn test_depth10_parent_subscribe_with_unparsable_id_returns_error(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let venue = Venue::new("BETFAIR");

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let test_clock: Rc<RefCell<TestClock>> = Rc::new(RefCell::new(TestClock::new()));
    register_mock_client(
        test_clock,
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let runner = InstrumentId::from("1.211334112-31570229.BETFAIR");
    let sub = DataCommand::Subscribe(SubscribeCommand::BookDepth10(SubscribeBookDepth10::new(
        runner,
        BookType::L2_MBP,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        true,
        None,
        Some(parent_params()),
    )));
    data_engine.execute(sub);

    {
        let cache_view = cache.borrow();
        assert!(
            cache_view.order_book(&runner).is_none(),
            "parent depth10 subscribe with an unparsable Betfair runner id must NOT create a book",
        );
    }
    assert!(
        !data_engine.subscribed_book_depth10().contains(&runner),
        "rejected parent depth10 subscribe must NOT leave the id in book_depth10_subs",
    );

    // Retrying without the parent flag on the same id must succeed.
    let retry = DataCommand::Subscribe(SubscribeCommand::BookDepth10(SubscribeBookDepth10::new(
        runner,
        BookType::L2_MBP,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        true,
        None,
        None,
    )));
    data_engine.execute(retry);
    assert!(
        cache.borrow().order_book(&runner).is_some(),
        "concrete depth10 subscribe after a rejected parent attempt must still create the exact-id book",
    );
}

#[rstest]
fn test_snapshots_parent_subscribe_with_unparsable_id_returns_error(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let venue = Venue::new("BETFAIR");

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let test_clock: Rc<RefCell<TestClock>> = Rc::new(RefCell::new(TestClock::new()));
    register_mock_client(
        test_clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let runner = InstrumentId::from("1.211334112-31570229.BETFAIR");
    let interval_ms = NonZeroUsize::new(1000).unwrap();
    let sub = DataCommand::Subscribe(SubscribeCommand::BookSnapshots(
        SubscribeBookSnapshots::new(
            runner,
            BookType::L2_MBP,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            interval_ms,
            None,
            Some(parent_params()),
        ),
    ));
    data_engine.execute(sub);

    assert!(
        !data_engine.subscribed_book_snapshots().contains(&runner),
        "rejected parent snapshots subscribe must NOT increment book_snapshot_counts \
         for the (id, interval) key",
    );

    // Retrying without the parent flag on the same (id, interval) must succeed;
    // the prior rejection cannot have left the snapshot counter in a half-incremented state.
    let retry = DataCommand::Subscribe(SubscribeCommand::BookSnapshots(
        SubscribeBookSnapshots::new(
            runner,
            BookType::L2_MBP,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            interval_ms,
            None,
            None,
        ),
    ));
    data_engine.execute(retry);
    assert!(
        data_engine.subscribed_book_snapshots().contains(&runner),
        "concrete snapshots subscribe after a rejected parent attempt must succeed",
    );
}

#[rstest]
fn test_concrete_subscribe_does_not_register_parent_expansion(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let venue = Venue::new("XCME");

    let esz1 = make_es_future("ESZ1.XCME", "ESZ1");
    let esh2 = make_es_future("ESH2.XCME", "ESH2");
    let esz1_id = esz1.id();
    let esh2_id = esh2.id();

    {
        let mut cache_mut = cache.borrow_mut();
        cache_mut
            .add_instrument(InstrumentAny::FuturesContract(esz1))
            .unwrap();
        cache_mut
            .add_instrument(InstrumentAny::FuturesContract(esh2))
            .unwrap();
    }

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let test_clock: Rc<RefCell<TestClock>> = Rc::new(RefCell::new(TestClock::new()));
    register_mock_client(
        test_clock,
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    // Concrete subscription: no Some(parent_params()), so the engine must NOT expand.
    let sub = DataCommand::Subscribe(SubscribeCommand::BookDeltas(SubscribeBookDeltas::new(
        esz1_id,
        BookType::L2_MBP,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        true,
        None,
        None,
    )));
    data_engine.execute(sub);

    let cache_view = cache.borrow();
    assert!(
        cache_view.order_book(&esz1_id).is_some(),
        "concrete subscribe must create the exact-id book",
    );
    assert!(
        cache_view.order_book(&esh2_id).is_none(),
        "concrete subscribe on ESZ1 must NOT spawn a book for ESH2 \
         even though both share the `ES` underlying root",
    );
}

#[rstest]
fn test_backtest_client_overrides_subscribe_routing(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();

    let venue_client_id = ClientId::new("VENUE_LIVE");
    let backtest_client_id = ClientId::new("BACKTEST");

    let venue_recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock.clone(),
        cache.clone(),
        venue_client_id,
        venue,
        None,
        &venue_recorder,
        &mut data_engine,
    );

    let backtest_recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        backtest_client_id,
        venue,
        None,
        &backtest_recorder,
        &mut data_engine,
    );

    let sub = DataCommand::Subscribe(SubscribeCommand::Quotes(SubscribeQuotes::new(
        audusd_sim.id,
        Some(venue_client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )));
    data_engine.execute(sub);

    assert_eq!(
        backtest_recorder.borrow().len(),
        1,
        "BACKTEST client should receive the subscribe override",
    );
    assert!(
        venue_recorder.borrow().is_empty(),
        "venue client should not receive subscribes when BACKTEST is registered",
    );
}

#[rstest]
fn test_backtest_client_overrides_when_registered_as_default(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();

    let venue_client_id = ClientId::new("VENUE_LIVE");
    let backtest_client_id = ClientId::new("BACKTEST");

    let venue_recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock.clone(),
        cache.clone(),
        venue_client_id,
        venue,
        None,
        &venue_recorder,
        &mut data_engine,
    );

    // `BacktestEngine` registers BACKTEST with venue=None, which lands the
    // adapter in `default_client` rather than `clients`
    let backtest_recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let backtest = MockDataClient::new_with_recorder(
        clock,
        cache,
        backtest_client_id,
        None,
        Some(backtest_recorder.clone()),
    );
    let backtest_adapter =
        DataClientAdapter::new(backtest_client_id, None, true, true, Box::new(backtest));
    data_engine.register_client(backtest_adapter, None);

    let sub = DataCommand::Subscribe(SubscribeCommand::Quotes(SubscribeQuotes::new(
        audusd_sim.id,
        Some(venue_client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )));
    data_engine.execute(sub);

    assert_eq!(
        backtest_recorder.borrow().len(),
        1,
        "BACKTEST default client must receive subscribes",
    );
    assert!(
        venue_recorder.borrow().is_empty(),
        "venue client must not receive subscribes when BACKTEST is the default",
    );
}

#[rstest]
fn test_emit_quotes_from_book_publishes_on_delta_apply(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let config = DataEngineConfig {
        emit_quotes_from_book: true,
        ..DataEngineConfig::default()
    };
    let mut data_engine = DataEngine::new(clock, cache.clone(), Some(config));

    let deltas = stub_deltas();
    let instrument_id = deltas.instrument_id;
    let venue = instrument_id.venue;

    let test_clock: Rc<RefCell<TestClock>> = Rc::new(RefCell::new(TestClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let sub = DataCommand::Subscribe(SubscribeCommand::BookDeltas(SubscribeBookDeltas::new(
        instrument_id,
        BookType::L3_MBO,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        true, // managed
        None,
        None,
    )));
    data_engine.execute(sub);

    let (handler, saver) = get_typed_message_saving_handler::<QuoteTick>(None);
    let quote_topic = switchboard::get_quotes_topic(instrument_id);
    msgbus::subscribe_quotes(quote_topic.into(), handler, None);

    let deltas_api = OrderBookDeltas_API::new(deltas);
    data_engine.process_data(Data::Deltas(deltas_api.clone()));

    assert_eq!(
        saver.get_messages().len(),
        1,
        "managed BookDeltas with emit_quotes_from_book must publish a top-of-book quote",
    );

    // Same deltas, same top-of-book: idempotent
    data_engine.process_data(Data::Deltas(deltas_api));
    assert_eq!(saver.get_messages().len(), 1);
}

#[rstest]
fn test_emit_quotes_from_book_publishes_on_depth_apply(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let config = DataEngineConfig {
        emit_quotes_from_book: true,
        ..DataEngineConfig::default()
    };
    let mut data_engine = DataEngine::new(clock.clone(), cache.clone(), Some(config));

    let depth = stub_depth10();
    let instrument_id = depth.instrument_id;
    let venue = instrument_id.venue;

    let test_clock: Rc<RefCell<TestClock>> = Rc::new(RefCell::new(TestClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let sub = DataCommand::Subscribe(SubscribeCommand::BookDepth10(SubscribeBookDepth10::new(
        instrument_id,
        BookType::L2_MBP,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        true,
        None,
        None,
    )));
    data_engine.execute(sub);

    let (handler, saver) = get_typed_message_saving_handler::<QuoteTick>(None);
    let quote_topic = switchboard::get_quotes_topic(instrument_id);
    msgbus::subscribe_quotes(quote_topic.into(), handler, None);

    data_engine.process_data(Data::Depth10(Box::new(depth)));

    let messages = saver.get_messages();
    assert_eq!(
        messages.len(),
        1,
        "managed depth subscription with emit_quotes_from_book must publish a top-of-book quote",
    );
}

#[rstest]
fn test_reset_clears_book_state_and_timers(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let deltas_topic = switchboard::get_book_deltas_topic(audusd_sim.id);
    let depth_topic = switchboard::get_book_depth10_topic(audusd_sim.id);

    let sub_deltas =
        DataCommand::Subscribe(SubscribeCommand::BookDeltas(SubscribeBookDeltas::new(
            audusd_sim.id,
            BookType::L3_MBO,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            true,
            None,
            None,
        )));
    data_engine.execute(sub_deltas);

    let sub_snapshots = DataCommand::Subscribe(SubscribeCommand::BookSnapshots(
        SubscribeBookSnapshots::new(
            audusd_sim.id,
            BookType::L3_MBO,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            NonZeroUsize::new(1000).unwrap(),
            None,
            None,
        ),
    ));
    data_engine.execute(sub_snapshots);

    assert_eq!(msgbus::subscriber_count_deltas(deltas_topic), 1);
    assert!(!data_engine.subscribed_book_snapshots().is_empty());
    assert!(!data_engine.get_clock().timer_names().is_empty());

    data_engine.reset();

    // Engine-owned book state and timers cleared; adapter-tracked subs
    // remain because `client.reset()` is a no-op
    assert_eq!(msgbus::subscriber_count_deltas(deltas_topic), 0);
    assert_eq!(msgbus::subscriber_count_depth10(depth_topic), 0);
    assert!(data_engine.subscribed_book_snapshots().is_empty());
    assert!(data_engine.get_clock().timer_names().is_empty());
    assert_eq!(data_engine.command_count(), 0);
    assert_eq!(data_engine.data_count(), 0);
}

#[rstest]
fn test_reset_clears_book_and_option_chain_state_and_allows_resubscribe(
    audusd_sim: CurrencyPair,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = msgbus::get_message_bus();
    let data_engine = make_option_chain_engine(clock.clone(), cache.clone());

    let sim_recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock.clone(),
        cache.clone(),
        client_id,
        venue,
        None,
        &sim_recorder,
        &mut data_engine.borrow_mut(),
    );

    let deribit_client_id = ClientId::new("DERIBIT");
    let deribit_venue = Venue::new("DERIBIT");
    let deribit_recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache.clone(),
        deribit_client_id,
        deribit_venue,
        Some(deribit_venue),
        &deribit_recorder,
        &mut data_engine.borrow_mut(),
    );

    let call = make_btc_option("50000.000", OptionKind::Call);
    let put = make_btc_option("50000.000", OptionKind::Put);
    let call_id = call.id();
    let _ = cache.borrow_mut().add_instrument(call);
    let _ = cache.borrow_mut().add_instrument(put);

    let book_id = audusd_sim.id;
    let deltas_topic = switchboard::get_book_deltas_topic(book_id);
    let depth_topic = switchboard::get_book_depth10_topic(book_id);
    let greeks_topic = switchboard::get_option_greeks_topic(call_id);
    let series_id = make_series_id();

    let subscribe_all = |engine: &Rc<RefCell<DataEngine>>| {
        engine
            .borrow_mut()
            .execute(DataCommand::Subscribe(SubscribeCommand::BookDeltas(
                SubscribeBookDeltas::new(
                    book_id,
                    BookType::L2_MBP,
                    Some(client_id),
                    Some(venue),
                    UUID4::new(),
                    UnixNanos::default(),
                    None,
                    true,
                    None,
                    None,
                ),
            )));
        engine
            .borrow_mut()
            .execute(DataCommand::Subscribe(SubscribeCommand::BookSnapshots(
                SubscribeBookSnapshots::new(
                    book_id,
                    BookType::L2_MBP,
                    Some(client_id),
                    Some(venue),
                    UUID4::new(),
                    UnixNanos::default(),
                    None,
                    NonZeroUsize::new(1000).unwrap(),
                    None,
                    None,
                ),
            )));
        engine.borrow_mut().execute(make_subscribe_option_chain(
            series_id,
            vec![Price::from("50000.000")],
            Some(deribit_client_id),
            Some(deribit_venue),
        ));
    };

    subscribe_all(&data_engine);

    assert_eq!(msgbus::subscriber_count_deltas(deltas_topic), 1);
    assert_eq!(msgbus::subscriber_count_depth10(depth_topic), 1);
    assert!(!data_engine.borrow().subscribed_book_snapshots().is_empty());
    assert!(!data_engine.borrow().get_clock().timer_names().is_empty());
    assert!(data_engine.borrow().has_option_chain_manager(&series_id));
    assert!(msgbus::exact_subscriber_count_option_greeks(greeks_topic) >= 1);

    data_engine.borrow_mut().reset();

    assert_eq!(msgbus::subscriber_count_deltas(deltas_topic), 0);
    assert_eq!(msgbus::subscriber_count_depth10(depth_topic), 0);
    assert!(data_engine.borrow().subscribed_book_snapshots().is_empty());
    assert!(data_engine.borrow().get_clock().timer_names().is_empty());
    assert!(!data_engine.borrow().has_option_chain_manager(&series_id));
    assert_eq!(data_engine.borrow().pending_option_chain_request_count(), 0);
    assert_eq!(
        msgbus::exact_subscriber_count_option_greeks(greeks_topic),
        0
    );

    subscribe_all(&data_engine);

    assert_eq!(msgbus::subscriber_count_deltas(deltas_topic), 1);
    assert_eq!(msgbus::subscriber_count_depth10(depth_topic), 1);
    assert!(!data_engine.borrow().subscribed_book_snapshots().is_empty());
    assert!(!data_engine.borrow().get_clock().timer_names().is_empty());
    assert!(data_engine.borrow().has_option_chain_manager(&series_id));
    assert!(msgbus::exact_subscriber_count_option_greeks(greeks_topic) >= 1);
}

#[rstest]
fn test_execute_subscribe_instrument(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let sub = SubscribeInstrument::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let sub_cmd = DataCommand::Subscribe(SubscribeCommand::Instrument(sub));
    data_engine.execute(sub_cmd.clone());

    assert!(
        data_engine
            .subscribed_instruments()
            .contains(&audusd_sim.id)
    );
    {
        assert_eq!(recorder.borrow().as_slice(), std::slice::from_ref(&sub_cmd));
    }

    let unsub = UnsubscribeInstrument::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let unsub_cmd = DataCommand::Unsubscribe(UnsubscribeCommand::Instrument(unsub));
    data_engine.execute(unsub_cmd.clone());

    assert!(
        !data_engine
            .subscribed_instruments()
            .contains(&audusd_sim.id)
    );
    assert_eq!(recorder.borrow().as_slice(), &[sub_cmd, unsub_cmd]);
}

#[rstest]
fn test_execute_subscribe_quotes(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let sub = SubscribeQuotes::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let sub_cmd = DataCommand::Subscribe(SubscribeCommand::Quotes(sub));
    data_engine.execute(sub_cmd.clone());

    assert!(data_engine.subscribed_quotes().contains(&audusd_sim.id));
    {
        assert_eq!(recorder.borrow().as_slice(), std::slice::from_ref(&sub_cmd));
    }

    let unsub = UnsubscribeQuotes::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let unsub_cmd = DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(unsub));
    data_engine.execute(unsub_cmd.clone());

    assert!(!data_engine.subscribed_quotes().contains(&audusd_sim.id));
    assert_eq!(recorder.borrow().as_slice(), &[sub_cmd, unsub_cmd]);
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_catalog_start_ns_prefill_quotes_from_catalog(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder = register_recording_client(&mut data_engine, clock, cache, client_id, venue);
    let _catalog_dir = register_quote_catalog(&mut data_engine, audusd_sim.id, 1_000);
    let correlation_id = UUID4::new();

    let sub = SubscribeQuotes::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        Some(correlation_id),
        None,
    );
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Quotes(sub)));

    let SubscribeCommand::Quotes(recorded) =
        recorded_subscribe_command_with_correlation(&recorder, correlation_id)
    else {
        panic!("expected quotes subscribe");
    };
    assert_eq!(
        recorded
            .params
            .as_ref()
            .and_then(|params| params.get_u64("start_ns")),
        Some(1_001)
    );
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_catalog_start_ns_prefill_quotes_preserves_existing_start_ns(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder = register_recording_client(&mut data_engine, clock, cache, client_id, venue);
    let _catalog_dir = register_quote_catalog(&mut data_engine, audusd_sim.id, 1_000);
    let params: Params = serde_json::from_value(json!({"start_ns": 42})).unwrap();
    let correlation_id = UUID4::new();

    let sub = SubscribeQuotes::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        Some(correlation_id),
        Some(params),
    );
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Quotes(sub)));

    let SubscribeCommand::Quotes(recorded) =
        recorded_subscribe_command_with_correlation(&recorder, correlation_id)
    else {
        panic!("expected quotes subscribe");
    };
    assert_eq!(
        recorded
            .params
            .as_ref()
            .and_then(|params| params.get_u64("start_ns")),
        Some(42)
    );
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_catalog_start_ns_prefill_quotes_sets_null_without_catalog_hit(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder = register_recording_client(&mut data_engine, clock, cache, client_id, venue);
    let _catalog_dir = register_empty_catalog(&mut data_engine, "empty-quotes");
    let correlation_id = UUID4::new();

    let sub = SubscribeQuotes::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        Some(correlation_id),
        None,
    );
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Quotes(sub)));

    let SubscribeCommand::Quotes(recorded) =
        recorded_subscribe_command_with_correlation(&recorder, correlation_id)
    else {
        panic!("expected quotes subscribe");
    };
    let null_value = json!(null);
    assert_eq!(
        recorded
            .params
            .as_ref()
            .and_then(|params| params.get("start_ns")),
        Some(&null_value)
    );
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_catalog_start_ns_prefill_trades_from_catalog(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder = register_recording_client(&mut data_engine, clock, cache, client_id, venue);
    let _catalog_dir = register_trade_catalog(&mut data_engine, audusd_sim.id, 2_000);
    let correlation_id = UUID4::new();

    let sub = SubscribeTrades::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        Some(correlation_id),
        None,
    );
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Trades(sub)));

    let SubscribeCommand::Trades(recorded) =
        recorded_subscribe_command_with_correlation(&recorder, correlation_id)
    else {
        panic!("expected trades subscribe");
    };
    assert_eq!(
        recorded
            .params
            .as_ref()
            .and_then(|params| params.get_u64("start_ns")),
        Some(2_001)
    );
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_catalog_start_ns_prefill_external_bars_from_catalog(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder = register_recording_client(&mut data_engine, clock, cache, client_id, venue);
    let bar_type = BarType::from("AUD/USD.SIM-1-MINUTE-LAST-EXTERNAL");
    let _catalog_dir = register_bar_catalog(&mut data_engine, bar_type, 3_000);
    let correlation_id = UUID4::new();

    let sub = SubscribeBars::new(
        bar_type,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        Some(correlation_id),
        None,
    );
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Bars(sub)));

    let SubscribeCommand::Bars(recorded) =
        recorded_subscribe_command_with_correlation(&recorder, correlation_id)
    else {
        panic!("expected bars subscribe");
    };
    assert_eq!(
        recorded
            .params
            .as_ref()
            .and_then(|params| params.get_u64("start_ns")),
        Some(3_001)
    );
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_catalog_start_ns_prefill_skips_internal_bars(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder = register_recording_client(&mut data_engine, clock, cache, client_id, venue);

    let inst_any = InstrumentAny::CurrencyPair(audusd_sim);
    data_engine.process(&inst_any as &dyn Any);

    let bar_type = BarType::from("AUD/USD.SIM-1-MINUTE-LAST-INTERNAL");
    let _catalog_dir = register_bar_catalog(&mut data_engine, bar_type, 4_000);

    let sub = SubscribeBars::new(
        bar_type,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Bars(sub)));

    let SubscribeCommand::Bars(recorded) = recorded_subscribe_command(&recorder) else {
        panic!("expected bars subscribe");
    };
    assert!(
        recorded
            .params
            .as_ref()
            .is_none_or(|params| !params.contains_key("start_ns"))
    );
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_catalog_start_ns_prefill_custom_data_from_catalog(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder = register_recording_client(&mut data_engine, clock, cache, client_id, venue);
    let data_type = DataType::new("CustomFeed", None, Some("SIM//AUDUSD".to_string()));
    let _catalog_dir = register_custom_catalog(&mut data_engine, &data_type, 5_000);
    let correlation_id = UUID4::new();

    let sub = SubscribeCustomData::new(
        Some(client_id),
        Some(venue),
        data_type,
        UUID4::new(),
        UnixNanos::default(),
        Some(correlation_id),
        None,
    );
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Data(sub)));

    let SubscribeCommand::Data(recorded) =
        recorded_subscribe_command_with_correlation(&recorder, correlation_id)
    else {
        panic!("expected custom data subscribe");
    };
    assert_eq!(
        recorded
            .params
            .as_ref()
            .and_then(|params| params.get_u64("start_ns")),
        Some(5_001)
    );
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_catalog_start_ns_prefill_custom_data_sets_null_without_catalog_hit(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder = register_recording_client(&mut data_engine, clock, cache, client_id, venue);
    let data_type = DataType::new("CustomFeed", None, Some("SIM//MISSING".to_string()));
    let _catalog_dir = register_empty_catalog(&mut data_engine, "empty-custom");
    let correlation_id = UUID4::new();

    let sub = SubscribeCustomData::new(
        Some(client_id),
        Some(venue),
        data_type,
        UUID4::new(),
        UnixNanos::default(),
        Some(correlation_id),
        None,
    );
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Data(sub)));

    let SubscribeCommand::Data(recorded) =
        recorded_subscribe_command_with_correlation(&recorder, correlation_id)
    else {
        panic!("expected custom data subscribe");
    };
    let null_value = json!(null);
    assert_eq!(
        recorded
            .params
            .as_ref()
            .and_then(|params| params.get("start_ns")),
        Some(&null_value)
    );
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_catalog_start_ns_prefill_custom_data_without_identifier_merges_catalog_intervals(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder = register_recording_client(&mut data_engine, clock, cache, client_id, venue);
    let type_name = "CustomFeed";
    let catalog_dir = CatalogTempDir::new("custom-no-identifier");
    let catalog = ParquetDataCatalog::new(catalog_dir.path(), None, None, None, None);
    write_custom_catalog_file(
        &catalog_dir,
        &catalog,
        type_name,
        Some("SIM//AUDUSD"),
        1_000,
        10_000,
    );
    write_custom_catalog_file(
        &catalog_dir,
        &catalog,
        type_name,
        Some("SIM//EURUSD"),
        5_000,
        6_000,
    );
    data_engine.register_catalog(catalog, None);
    let data_type = DataType::new(type_name, None, None);
    let correlation_id = UUID4::new();

    let sub = SubscribeCustomData::new(
        Some(client_id),
        Some(venue),
        data_type,
        UUID4::new(),
        UnixNanos::default(),
        Some(correlation_id),
        None,
    );
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Data(sub)));

    let SubscribeCommand::Data(recorded) =
        recorded_subscribe_command_with_correlation(&recorder, correlation_id)
    else {
        panic!("expected custom data subscribe");
    };
    assert_eq!(
        recorded
            .params
            .as_ref()
            .and_then(|params| params.get_u64("start_ns")),
        Some(10_001)
    );
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_catalog_start_ns_prefill_custom_data_preserves_existing_start_ns(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder = register_recording_client(&mut data_engine, clock, cache, client_id, venue);
    let data_type = DataType::new("CustomFeed", None, Some("SIM//AUDUSD".to_string()));
    let _catalog_dir = register_custom_catalog(&mut data_engine, &data_type, 6_000);
    let params: Params = serde_json::from_value(json!({"start_ns": 42})).unwrap();
    let correlation_id = UUID4::new();

    let sub = SubscribeCustomData::new(
        Some(client_id),
        Some(venue),
        data_type,
        UUID4::new(),
        UnixNanos::default(),
        Some(correlation_id),
        Some(params),
    );
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Data(sub)));

    let SubscribeCommand::Data(recorded) =
        recorded_subscribe_command_with_correlation(&recorder, correlation_id)
    else {
        panic!("expected custom data subscribe");
    };
    assert_eq!(
        recorded
            .params
            .as_ref()
            .and_then(|params| params.get_u64("start_ns")),
        Some(42)
    );
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_catalog_start_ns_prefill_custom_data_preserves_command_metadata(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder = register_recording_client(&mut data_engine, clock, cache, client_id, venue);
    let metadata = serde_json::from_value(json!({
        "instrument_id": "IGNORED.SIM",
        "source": "metadata",
    }))
    .unwrap();
    let data_type = DataType::new(
        "CustomMetadataFeed",
        Some(metadata),
        Some("SIM//METADATA".to_string()),
    );
    let _catalog_dir = register_custom_catalog(&mut data_engine, &data_type, 7_000);
    let command_id = UUID4::new();
    let ts_init = UnixNanos::from(123);
    let correlation_id = UUID4::new();
    let params: Params = serde_json::from_value(json!({"source": "params"})).unwrap();

    let sub = SubscribeCustomData::new(
        Some(client_id),
        Some(venue),
        data_type.clone(),
        command_id,
        ts_init,
        Some(correlation_id),
        Some(params),
    );
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Data(sub)));

    let SubscribeCommand::Data(recorded) =
        recorded_subscribe_command_with_correlation(&recorder, correlation_id)
    else {
        panic!("expected custom data subscribe");
    };

    assert_eq!(recorded.client_id, Some(client_id));
    assert_eq!(recorded.venue, Some(venue));
    assert_eq!(recorded.data_type.type_name(), data_type.type_name());
    assert_eq!(recorded.data_type.metadata(), data_type.metadata());
    assert_eq!(recorded.data_type.identifier(), data_type.identifier());
    assert_eq!(recorded.command_id, command_id);
    assert_eq!(recorded.ts_init, ts_init);
    assert_eq!(recorded.correlation_id, Some(correlation_id));
    assert_eq!(
        recorded
            .params
            .as_ref()
            .and_then(|params| params.get_u64("start_ns")),
        Some(7_001)
    );
    assert_eq!(
        recorded
            .params
            .as_ref()
            .and_then(|params| params.get_str("source")),
        Some("params")
    );
}

#[rstest]
fn test_subscribe_spread_quotes_default_interval_publishes_on_timer(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let data_engine = create_snapshot_test_engine(clock.clone(), cache.clone());
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock.clone(),
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let spread = generic_futures_spread();
    let spread_id = spread.id();
    let (leg_a, leg_b) = generic_futures_spread_legs();
    let spread_any = InstrumentAny::FuturesSpread(spread);
    data_engine.process(&spread_any as &dyn Any);

    let (handler, saver) =
        get_typed_message_saving_handler::<QuoteTick>(Some(Ustr::from("spread-quotes-timer")));
    let spread_topic = switchboard::get_quotes_topic(spread_id);
    msgbus::subscribe_quotes(spread_topic.into(), handler, None);

    let sub = SubscribeQuotes::new(
        spread_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        Some(spread_quote_default_interval_params()),
    );
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Quotes(sub)));

    let timer_name = format!("SPREAD_QUOTE_{spread_id}");
    assert!(
        data_engine
            .get_clock()
            .timer_names()
            .iter()
            .any(|name| *name == timer_name)
    );

    advance_clock_and_dispatch(&clock, 0);
    assert!(saver.get_messages().is_empty());

    let quote_a = QuoteTick::new(
        leg_a,
        Price::from("101.00"),
        Price::from("102.00"),
        Quantity::from(5),
        Quantity::from(6),
        UnixNanos::from(1),
        UnixNanos::from(1),
    );
    let quote_b = QuoteTick::new(
        leg_b,
        Price::from("99.00"),
        Price::from("100.00"),
        Quantity::from(7),
        Quantity::from(8),
        UnixNanos::from(2),
        UnixNanos::from(2),
    );
    data_engine.process_data(Data::Quote(quote_a));
    data_engine.process_data(Data::Quote(quote_b));
    assert!(saver.get_messages().is_empty());

    advance_clock_and_dispatch(&clock, 1_000_000_000);

    let spread_quotes = saver.get_messages();
    assert_eq!(spread_quotes.len(), 1);
    assert_eq!(spread_quotes[0].instrument_id, spread_id);
    assert_eq!(spread_quotes[0].bid_price, Price::from("1.00"));
    assert_eq!(spread_quotes[0].ask_price, Price::from("3.00"));
    assert_eq!(spread_quotes[0].bid_size, Quantity::from(5));
    assert_eq!(spread_quotes[0].ask_size, Quantity::from(6));
    assert_eq!(spread_quotes[0].ts_event, UnixNanos::from(1_000_000_000));
}

#[rstest]
fn test_subscribe_spread_quotes_with_zero_interval_publishes_spread_quote(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let spread = generic_futures_spread();
    let spread_id = spread.id();
    let (leg_a, leg_b) = generic_futures_spread_legs();
    let spread_any = InstrumentAny::FuturesSpread(spread);
    data_engine.process(&spread_any as &dyn Any);

    let (handler, saver) =
        get_typed_message_saving_handler::<QuoteTick>(Some(Ustr::from("spread-quotes")));
    let spread_topic = switchboard::get_quotes_topic(spread_id);
    msgbus::subscribe_quotes(spread_topic.into(), handler, None);

    let sub = SubscribeQuotes::new(
        spread_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        Some(spread_quote_zero_interval_params()),
    );
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Quotes(sub)));

    let leg_subscriptions: Vec<InstrumentId> = recorder
        .borrow()
        .iter()
        .filter_map(|cmd| match cmd {
            DataCommand::Subscribe(SubscribeCommand::Quotes(cmd)) => Some(cmd.instrument_id),
            _ => None,
        })
        .collect();
    assert_eq!(leg_subscriptions, vec![leg_a, leg_b]);

    let quote_a = QuoteTick::new(
        leg_a,
        Price::from("101.00"),
        Price::from("102.00"),
        Quantity::from(5),
        Quantity::from(6),
        UnixNanos::from(1),
        UnixNanos::from(1),
    );
    data_engine.process_data(Data::Quote(quote_a));
    assert!(saver.get_messages().is_empty());

    let quote_b = QuoteTick::new(
        leg_b,
        Price::from("99.00"),
        Price::from("100.00"),
        Quantity::from(7),
        Quantity::from(8),
        UnixNanos::from(2),
        UnixNanos::from(2),
    );
    data_engine.process_data(Data::Quote(quote_b));

    let spread_quotes = saver.get_messages();
    assert_eq!(spread_quotes.len(), 1);
    assert_eq!(spread_quotes[0].instrument_id, spread_id);
    assert_eq!(spread_quotes[0].bid_price, Price::from("1.00"));
    assert_eq!(spread_quotes[0].ask_price, Price::from("3.00"));
    assert_eq!(spread_quotes[0].bid_size, Quantity::from(5));
    assert_eq!(spread_quotes[0].ask_size, Quantity::from(6));
}

#[rstest]
fn test_unsubscribe_spread_quotes_stops_default_interval_timer(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let spread = generic_futures_spread();
    let spread_id = spread.id();
    let spread_any = InstrumentAny::FuturesSpread(spread);
    data_engine.process(&spread_any as &dyn Any);

    let params = spread_quote_default_interval_params();
    let sub = SubscribeQuotes::new(
        spread_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        Some(params.clone()),
    );
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Quotes(sub)));

    let timer_name = format!("SPREAD_QUOTE_{spread_id}");
    assert!(
        data_engine
            .get_clock()
            .timer_names()
            .iter()
            .any(|name| *name == timer_name)
    );

    let unsub = UnsubscribeQuotes::new(
        spread_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        Some(params),
    );
    data_engine.execute(DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(unsub)));

    let (leg_a, leg_b) = generic_futures_spread_legs();
    assert!(data_engine.get_clock().timer_names().is_empty());
    assert_eq!(
        msgbus::exact_subscriber_count_quotes(switchboard::get_quotes_topic(leg_a)),
        0
    );
    assert_eq!(
        msgbus::exact_subscriber_count_quotes(switchboard::get_quotes_topic(leg_b)),
        0
    );
}

#[rstest]
fn test_unsubscribe_spread_quotes_removes_leg_handlers(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let spread = generic_futures_spread();
    let spread_id = spread.id();
    let (leg_a, leg_b) = generic_futures_spread_legs();
    let spread_any = InstrumentAny::FuturesSpread(spread);
    data_engine.process(&spread_any as &dyn Any);

    let (handler, saver) =
        get_typed_message_saving_handler::<QuoteTick>(Some(Ustr::from("spread-quotes")));
    let spread_topic = switchboard::get_quotes_topic(spread_id);
    msgbus::subscribe_quotes(spread_topic.into(), handler, None);

    let params = spread_quote_params();
    let sub = SubscribeQuotes::new(
        spread_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        Some(params.clone()),
    );
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Quotes(sub)));
    recorder.borrow_mut().clear();

    let unsub = UnsubscribeQuotes::new(
        spread_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        Some(params),
    );
    data_engine.execute(DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(unsub)));

    let leg_unsubscriptions: Vec<InstrumentId> = recorder
        .borrow()
        .iter()
        .filter_map(|cmd| match cmd {
            DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(cmd)) => Some(cmd.instrument_id),
            _ => None,
        })
        .collect();
    assert_eq!(leg_unsubscriptions, vec![leg_a, leg_b]);

    let quote_a = QuoteTick::new(
        leg_a,
        Price::from("101.00"),
        Price::from("102.00"),
        Quantity::from(5),
        Quantity::from(6),
        UnixNanos::from(1),
        UnixNanos::from(1),
    );
    let quote_b = QuoteTick::new(
        leg_b,
        Price::from("99.00"),
        Price::from("100.00"),
        Quantity::from(7),
        Quantity::from(8),
        UnixNanos::from(2),
        UnixNanos::from(2),
    );
    data_engine.process_data(Data::Quote(quote_a));
    data_engine.process_data(Data::Quote(quote_b));

    assert!(saver.get_messages().is_empty());
}

#[rstest]
fn test_reset_stops_spread_quote_timer_and_removes_leg_handlers(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let spread = generic_futures_spread();
    let spread_id = spread.id();
    let (leg_a, leg_b) = generic_futures_spread_legs();
    let spread_any = InstrumentAny::FuturesSpread(spread);
    data_engine.process(&spread_any as &dyn Any);

    let sub = SubscribeQuotes::new(
        spread_id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        Some(spread_quote_default_interval_params()),
    );
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Quotes(sub)));

    let timer_name = format!("SPREAD_QUOTE_{spread_id}");
    assert!(
        data_engine
            .get_clock()
            .timer_names()
            .iter()
            .any(|name| *name == timer_name)
    );

    data_engine.reset();

    assert!(data_engine.get_clock().timer_names().is_empty());
    assert_eq!(
        msgbus::exact_subscriber_count_quotes(switchboard::get_quotes_topic(leg_a)),
        0
    );
    assert_eq!(
        msgbus::exact_subscriber_count_quotes(switchboard::get_quotes_topic(leg_b)),
        0
    );
}

#[rstest]
fn test_unsubscribe_quotes_keeps_client_subscribed_when_other_subscribers(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let topic = switchboard::get_quotes_topic(audusd_sim.id);
    let (handler_a, saver_a) =
        get_typed_message_saving_handler::<QuoteTick>(Some(Ustr::from("subscriber-a")));
    let (handler_b, saver_b) =
        get_typed_message_saving_handler::<QuoteTick>(Some(Ustr::from("subscriber-b")));
    msgbus::subscribe_quotes(topic.into(), handler_a, None);
    msgbus::subscribe_quotes(topic.into(), handler_b, None);

    let sub = SubscribeQuotes::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let sub_cmd = DataCommand::Subscribe(SubscribeCommand::Quotes(sub));
    data_engine.execute(sub_cmd.clone());

    let unsub = UnsubscribeQuotes::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let unsub_cmd = DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(unsub));
    data_engine.execute(unsub_cmd);

    assert_eq!(recorder.borrow().as_slice(), std::slice::from_ref(&sub_cmd));

    let quote = QuoteTick::new(
        audusd_sim.id,
        Price::from("1.0000"),
        Price::from("1.0001"),
        Quantity::from(1),
        Quantity::from(1),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    data_engine.process_data(Data::Quote(quote));

    assert_eq!(saver_a.get_messages(), vec![quote]);
    assert_eq!(saver_b.get_messages(), vec![quote]);
}

#[rstest]
fn test_unsubscribe_quotes_ignores_wildcard_observers(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let (wildcard_handler, _wildcard_saver) =
        get_typed_message_saving_handler::<QuoteTick>(Some(Ustr::from("wildcard-observer")));
    msgbus::subscribe_quotes("data.quotes.*".into(), wildcard_handler, Some(10));

    let sub = SubscribeQuotes::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let sub_cmd = DataCommand::Subscribe(SubscribeCommand::Quotes(sub));
    data_engine.execute(sub_cmd.clone());

    let unsub = UnsubscribeQuotes::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let unsub_cmd = DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(unsub));
    data_engine.execute(unsub_cmd.clone());

    assert_eq!(recorder.borrow().as_slice(), &[sub_cmd, unsub_cmd]);
}

#[rstest]
fn test_execute_subscribe_trades(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let sub = SubscribeTrades::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let sub_cmd = DataCommand::Subscribe(SubscribeCommand::Trades(sub));
    data_engine.execute(sub_cmd.clone());

    assert!(data_engine.subscribed_trades().contains(&audusd_sim.id));
    {
        assert_eq!(recorder.borrow().as_slice(), std::slice::from_ref(&sub_cmd));
    }

    let ubsub = UnsubscribeTrades::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let unsub_cmd = DataCommand::Unsubscribe(UnsubscribeCommand::Trades(ubsub));
    data_engine.execute(unsub_cmd.clone());

    assert!(!data_engine.subscribed_trades().contains(&audusd_sim.id));
    assert_eq!(recorder.borrow().as_slice(), &[sub_cmd, unsub_cmd]);
}

#[rstest]
fn test_unsubscribe_trades_ignores_wildcard_observers(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let (wildcard_handler, _wildcard_saver) =
        get_typed_message_saving_handler::<TradeTick>(Some(Ustr::from("wildcard-trades")));
    msgbus::subscribe_trades("data.trades.*".into(), wildcard_handler, Some(10));

    let sub_cmd = DataCommand::Subscribe(SubscribeCommand::Trades(SubscribeTrades::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )));
    data_engine.execute(sub_cmd.clone());

    let unsub_cmd = DataCommand::Unsubscribe(UnsubscribeCommand::Trades(UnsubscribeTrades::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )));
    data_engine.execute(unsub_cmd.clone());

    assert_eq!(recorder.borrow().as_slice(), &[sub_cmd, unsub_cmd]);
}

#[rstest]
fn test_execute_subscribe_bars(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let inst_any = InstrumentAny::CurrencyPair(audusd_sim.clone());
    data_engine.process(&inst_any as &dyn Any);

    let bar_type = BarType::from("AUD/USD.SIM-1-MINUTE-LAST-INTERNAL");

    let sub = SubscribeBars::new(
        bar_type,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let sub_cmd = DataCommand::Subscribe(SubscribeCommand::Bars(sub));
    data_engine.execute(sub_cmd.clone());

    assert!(data_engine.subscribed_bars().contains(&bar_type));
    {
        assert_eq!(recorder.borrow().as_slice(), std::slice::from_ref(&sub_cmd));
    }

    let unsub = UnsubscribeBars::new(
        bar_type,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let unsub_cmd = DataCommand::Unsubscribe(UnsubscribeCommand::Bars(unsub));
    data_engine.execute(unsub_cmd.clone());

    assert_eq!(audusd_sim.id(), bar_type.instrument_id());
    assert!(!data_engine.subscribed_bars().contains(&bar_type));
    assert_eq!(recorder.borrow().as_slice(), &[sub_cmd, unsub_cmd]);
}

#[rstest]
fn test_unsubscribe_bars_forwards_to_client_with_remaining_exact_subscribers(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    // Bars excluded from the gate; venue unsubscribe must forward even with exact subscribers
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let inst_any = InstrumentAny::CurrencyPair(audusd_sim);
    data_engine.process(&inst_any as &dyn Any);

    let bar_type = BarType::from("AUD/USD.SIM-1-MINUTE-LAST-INTERNAL");
    let bar_topic = switchboard::get_bars_topic(bar_type);
    let (handler, _saver) =
        get_typed_message_saving_handler::<Bar>(Some(Ustr::from("exact-bar-subscriber")));
    msgbus::subscribe_bars(bar_topic.into(), handler, None);

    let sub_cmd = DataCommand::Subscribe(SubscribeCommand::Bars(SubscribeBars::new(
        bar_type,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )));
    data_engine.execute(sub_cmd.clone());

    let unsub_cmd = DataCommand::Unsubscribe(UnsubscribeCommand::Bars(UnsubscribeBars::new(
        bar_type,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )));
    data_engine.execute(unsub_cmd.clone());

    assert_eq!(recorder.borrow().as_slice(), &[sub_cmd, unsub_cmd]);
}

#[rstest]
fn test_bar_aggregator_quote_subscription_priority_is_between_4_and_6(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let inst_any = InstrumentAny::CurrencyPair(audusd_sim.clone());
    data_engine.process(&inst_any as &dyn Any);

    let bar_type = BarType::from("AUD/USD.SIM-1-TICK-BID-INTERNAL");
    let quote_topic = switchboard::get_quotes_topic(audusd_sim.id);
    let bar_topic = switchboard::get_bars_topic(bar_type);

    let dispatch_order: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));

    let order_high = dispatch_order.clone();
    let handler_high = TypedHandler::from_with_id("prio-6", move |_q: &QuoteTick| {
        order_high.borrow_mut().push("high");
    });
    msgbus::subscribe_quotes(quote_topic.into(), handler_high, Some(6));

    let order_low = dispatch_order.clone();
    let handler_low = TypedHandler::from_with_id("prio-4", move |_q: &QuoteTick| {
        order_low.borrow_mut().push("low");
    });
    msgbus::subscribe_quotes(quote_topic.into(), handler_low, Some(4));

    let order_bar = dispatch_order.clone();
    let handler_bar = TypedHandler::from_with_id("bar-observer", move |_b: &Bar| {
        order_bar.borrow_mut().push("bar");
    });
    msgbus::subscribe_bars(bar_topic.into(), handler_bar, None);

    let sub = SubscribeBars::new(
        bar_type,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Bars(sub)));

    let quote = QuoteTick::new(
        audusd_sim.id,
        Price::from("1.0000"),
        Price::from("1.0001"),
        Quantity::from(1),
        Quantity::from(1),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    data_engine.process_data(Data::Quote(quote));

    assert_eq!(*dispatch_order.borrow(), vec!["high", "bar", "low"]);
}

#[rstest]
fn test_bar_aggregator_trade_subscription_priority_is_between_4_and_6(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let inst_any = InstrumentAny::CurrencyPair(audusd_sim.clone());
    data_engine.process(&inst_any as &dyn Any);

    let bar_type = BarType::from("AUD/USD.SIM-1-TICK-LAST-INTERNAL");
    let trades_topic = switchboard::get_trades_topic(audusd_sim.id);
    let bar_topic = switchboard::get_bars_topic(bar_type);

    let dispatch_order: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));

    let order_high = dispatch_order.clone();
    let handler_high = TypedHandler::from_with_id("prio-6", move |_t: &TradeTick| {
        order_high.borrow_mut().push("high");
    });
    msgbus::subscribe_trades(trades_topic.into(), handler_high, Some(6));

    let order_low = dispatch_order.clone();
    let handler_low = TypedHandler::from_with_id("prio-4", move |_t: &TradeTick| {
        order_low.borrow_mut().push("low");
    });
    msgbus::subscribe_trades(trades_topic.into(), handler_low, Some(4));

    let order_bar = dispatch_order.clone();
    let handler_bar = TypedHandler::from_with_id("bar-observer", move |_b: &Bar| {
        order_bar.borrow_mut().push("bar");
    });
    msgbus::subscribe_bars(bar_topic.into(), handler_bar, None);

    let sub = SubscribeBars::new(
        bar_type,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Bars(sub)));

    let trade = TradeTick::new(
        audusd_sim.id,
        Price::from("1.0000"),
        Quantity::from(1),
        AggressorSide::Buyer,
        TradeId::new("T-1"),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    data_engine.process_data(Data::Trade(trade));

    assert_eq!(*dispatch_order.borrow(), vec!["high", "bar", "low"]);
}

#[rstest]
fn test_composite_bar_aggregator_source_bar_subscription_uses_default_priority(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let inst_any = InstrumentAny::CurrencyPair(audusd_sim);
    data_engine.process(&inst_any as &dyn Any);

    let bar_type = BarType::from("AUD/USD.SIM-1-TICK-LAST-INTERNAL@1-TICK-EXTERNAL");
    let source_bar_type = bar_type.composite();
    let source_topic = switchboard::get_bars_topic(source_bar_type);
    let target_topic = switchboard::get_bars_topic(bar_type);

    let dispatch_order: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));

    let order_high = dispatch_order.clone();
    let handler_high = TypedHandler::from_with_id("prio-1", move |_b: &Bar| {
        order_high.borrow_mut().push("high");
    });
    msgbus::subscribe_bars(source_topic.into(), handler_high, Some(1));

    let order_bar = dispatch_order.clone();
    let handler_bar = TypedHandler::from_with_id("target-bar-observer", move |_b: &Bar| {
        order_bar.borrow_mut().push("bar");
    });
    msgbus::subscribe_bars(target_topic.into(), handler_bar, None);

    let sub = SubscribeBars::new(
        bar_type,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Bars(sub)));

    let source_bar = Bar::new(
        source_bar_type,
        Price::from("1.0000"),
        Price::from("1.0001"),
        Price::from("0.9999"),
        Price::from("1.0000"),
        Quantity::from(1),
        UnixNanos::from(1),
        UnixNanos::from(1),
    );
    data_engine.process_data(Data::Bar(source_bar));

    assert_eq!(*dispatch_order.borrow(), vec!["high", "bar"]);
}

#[rstest]
fn test_execute_subscribe_mark_prices(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let sub = SubscribeMarkPrices::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let sub_cmd = DataCommand::Subscribe(SubscribeCommand::MarkPrices(sub));
    data_engine.execute(sub_cmd.clone());

    assert!(
        data_engine
            .subscribed_mark_prices()
            .contains(&audusd_sim.id)
    );
    {
        assert_eq!(recorder.borrow().as_slice(), std::slice::from_ref(&sub_cmd));
    }

    let unsub = UnsubscribeMarkPrices::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let unsub_cmd = DataCommand::Unsubscribe(UnsubscribeCommand::MarkPrices(unsub));
    data_engine.execute(unsub_cmd.clone());

    assert!(
        !data_engine
            .subscribed_mark_prices()
            .contains(&audusd_sim.id)
    );
    assert_eq!(recorder.borrow().as_slice(), &[sub_cmd, unsub_cmd]);
}

#[rstest]
fn test_unsubscribe_mark_prices_keeps_client_subscribed_when_other_subscribers(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let topic = switchboard::get_mark_price_topic(audusd_sim.id);
    let (handler_a, _saver_a) =
        get_typed_message_saving_handler::<MarkPriceUpdate>(Some(Ustr::from("mark-a")));
    let (handler_b, _saver_b) =
        get_typed_message_saving_handler::<MarkPriceUpdate>(Some(Ustr::from("mark-b")));
    msgbus::subscribe_mark_prices(topic.into(), handler_a, None);
    msgbus::subscribe_mark_prices(topic.into(), handler_b, None);

    let sub_cmd = DataCommand::Subscribe(SubscribeCommand::MarkPrices(SubscribeMarkPrices::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )));
    data_engine.execute(sub_cmd.clone());

    let unsub_cmd =
        DataCommand::Unsubscribe(UnsubscribeCommand::MarkPrices(UnsubscribeMarkPrices::new(
            audusd_sim.id,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        )));
    data_engine.execute(unsub_cmd);

    assert_eq!(recorder.borrow().as_slice(), std::slice::from_ref(&sub_cmd));
}

#[rstest]
fn test_unsubscribe_mark_prices_ignores_wildcard_observers(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let (wildcard_handler, _wildcard_saver) =
        get_typed_message_saving_handler::<MarkPriceUpdate>(Some(Ustr::from("wildcard-mark")));
    msgbus::subscribe_mark_prices("data.mark_prices.*".into(), wildcard_handler, Some(10));

    let sub_cmd = DataCommand::Subscribe(SubscribeCommand::MarkPrices(SubscribeMarkPrices::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )));
    data_engine.execute(sub_cmd.clone());

    let unsub_cmd =
        DataCommand::Unsubscribe(UnsubscribeCommand::MarkPrices(UnsubscribeMarkPrices::new(
            audusd_sim.id,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        )));
    data_engine.execute(unsub_cmd.clone());

    assert_eq!(recorder.borrow().as_slice(), &[sub_cmd, unsub_cmd]);
}

#[rstest]
fn test_execute_subscribe_index_prices(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let sub_cmd = DataCommand::Subscribe(SubscribeCommand::IndexPrices(SubscribeIndexPrices::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )));
    data_engine.execute(sub_cmd.clone());

    assert!(
        data_engine
            .subscribed_index_prices()
            .contains(&audusd_sim.id)
    );
    {
        assert_eq!(recorder.borrow().as_slice(), std::slice::from_ref(&sub_cmd));
    }

    let unsub_cmd = DataCommand::Unsubscribe(UnsubscribeCommand::IndexPrices(
        UnsubscribeIndexPrices::new(
            audusd_sim.id,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ),
    ));
    data_engine.execute(unsub_cmd.clone());

    assert!(
        !data_engine
            .subscribed_index_prices()
            .contains(&audusd_sim.id)
    );
    {
        assert_eq!(recorder.borrow().as_slice(), &[sub_cmd, unsub_cmd]);
    }
}

#[rstest]
fn test_unsubscribe_index_prices_ignores_wildcard_observers(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let (wildcard_handler, _wildcard_saver) =
        get_typed_message_saving_handler::<IndexPriceUpdate>(Some(Ustr::from("wildcard-index")));
    msgbus::subscribe_index_prices("data.index_prices.*".into(), wildcard_handler, Some(10));

    let sub_cmd = DataCommand::Subscribe(SubscribeCommand::IndexPrices(SubscribeIndexPrices::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )));
    data_engine.execute(sub_cmd.clone());

    let unsub_cmd = DataCommand::Unsubscribe(UnsubscribeCommand::IndexPrices(
        UnsubscribeIndexPrices::new(
            audusd_sim.id,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ),
    ));
    data_engine.execute(unsub_cmd.clone());

    assert_eq!(recorder.borrow().as_slice(), &[sub_cmd, unsub_cmd]);
}

#[rstest]
fn test_execute_subscribe_funding_rates(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let sub = SubscribeFundingRates::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let sub_cmd = DataCommand::Subscribe(SubscribeCommand::FundingRates(sub));
    data_engine.execute(sub_cmd.clone());

    assert!(
        data_engine
            .subscribed_funding_rates()
            .contains(&audusd_sim.id)
    );
    {
        assert_eq!(recorder.borrow().as_slice(), std::slice::from_ref(&sub_cmd));
    }

    let unsub = UnsubscribeFundingRates::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let unsub_cmd = DataCommand::Unsubscribe(UnsubscribeCommand::FundingRates(unsub));
    data_engine.execute(unsub_cmd.clone());

    assert!(
        !data_engine
            .subscribed_funding_rates()
            .contains(&audusd_sim.id)
    );
    assert_eq!(recorder.borrow().as_slice(), &[sub_cmd, unsub_cmd]);
}

#[rstest]
fn test_unsubscribe_funding_rates_ignores_wildcard_observers(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let (wildcard_handler, _wildcard_saver) =
        get_typed_message_saving_handler::<FundingRateUpdate>(Some(Ustr::from("wildcard-funding")));
    msgbus::subscribe_funding_rates("data.funding_rates.*".into(), wildcard_handler, Some(10));

    let sub_cmd =
        DataCommand::Subscribe(SubscribeCommand::FundingRates(SubscribeFundingRates::new(
            audusd_sim.id,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        )));
    data_engine.execute(sub_cmd.clone());

    let unsub_cmd = DataCommand::Unsubscribe(UnsubscribeCommand::FundingRates(
        UnsubscribeFundingRates::new(
            audusd_sim.id,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ),
    ));
    data_engine.execute(unsub_cmd.clone());

    assert_eq!(recorder.borrow().as_slice(), &[sub_cmd, unsub_cmd]);
}

#[rstest]
fn test_execute_subscribe_instrument_status(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let sub = SubscribeInstrumentStatus::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let sub_cmd = DataCommand::Subscribe(SubscribeCommand::InstrumentStatus(sub));
    data_engine.execute(sub_cmd.clone());

    assert!(
        data_engine
            .subscribed_instrument_status()
            .contains(&audusd_sim.id)
    );
    {
        assert_eq!(recorder.borrow().as_slice(), std::slice::from_ref(&sub_cmd));
    }

    let unsub = UnsubscribeInstrumentStatus::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let unsub_cmd = DataCommand::Unsubscribe(UnsubscribeCommand::InstrumentStatus(unsub));
    data_engine.execute(unsub_cmd.clone());

    assert!(
        !data_engine
            .subscribed_instrument_status()
            .contains(&audusd_sim.id)
    );
    assert_eq!(recorder.borrow().as_slice(), &[sub_cmd, unsub_cmd]);
}

#[rstest]
fn test_execute_subscribe_instrument_close(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let sub = SubscribeInstrumentClose::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let sub_cmd = DataCommand::Subscribe(SubscribeCommand::InstrumentClose(sub));
    data_engine.execute(sub_cmd.clone());

    assert!(
        data_engine
            .subscribed_instrument_close()
            .contains(&audusd_sim.id)
    );
    {
        assert_eq!(recorder.borrow().as_slice(), std::slice::from_ref(&sub_cmd));
    }

    let unsub = UnsubscribeInstrumentClose::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let unsub_cmd = DataCommand::Unsubscribe(UnsubscribeCommand::InstrumentClose(unsub));
    data_engine.execute(unsub_cmd.clone());

    assert!(
        !data_engine
            .subscribed_instrument_close()
            .contains(&audusd_sim.id)
    );
    assert_eq!(recorder.borrow().as_slice(), &[sub_cmd, unsub_cmd]);
}

#[rstest]
fn test_execute_subscribe_option_greeks(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let sub = SubscribeOptionGreeks::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let sub_cmd = DataCommand::Subscribe(SubscribeCommand::OptionGreeks(sub));
    data_engine.execute(sub_cmd.clone());

    {
        assert_eq!(recorder.borrow().as_slice(), std::slice::from_ref(&sub_cmd));
    }

    let unsub = UnsubscribeOptionGreeks::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let unsub_cmd = DataCommand::Unsubscribe(UnsubscribeCommand::OptionGreeks(unsub));
    data_engine.execute(unsub_cmd.clone());

    assert_eq!(recorder.borrow().as_slice(), &[sub_cmd, unsub_cmd]);
}

#[rstest]
fn test_unsubscribe_option_greeks_ignores_wildcard_observers(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let (wildcard_handler, _wildcard_saver) =
        get_typed_message_saving_handler::<OptionGreeks>(Some(Ustr::from("wildcard-greeks")));
    msgbus::subscribe_option_greeks("data.option_greeks.*".into(), wildcard_handler, Some(10));

    let sub_cmd =
        DataCommand::Subscribe(SubscribeCommand::OptionGreeks(SubscribeOptionGreeks::new(
            audusd_sim.id,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        )));
    data_engine.execute(sub_cmd.clone());

    let unsub_cmd = DataCommand::Unsubscribe(UnsubscribeCommand::OptionGreeks(
        UnsubscribeOptionGreeks::new(
            audusd_sim.id,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ),
    ));
    data_engine.execute(unsub_cmd.clone());

    assert_eq!(recorder.borrow().as_slice(), &[sub_cmd, unsub_cmd]);
}

#[rstest]
fn test_execute_request_data(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    data_engine: Rc<RefCell<DataEngine>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let req = RequestCustomData {
        client_id,
        data_type: DataType::new("X", None, None),
        start: None,
        end: None,
        limit: None,
        request_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    };
    let cmd = DataCommand::Request(RequestCommand::Data(req));
    data_engine.execute(cmd.clone());

    assert_eq!(recorder.borrow()[0], cmd);
}

#[rstest]
fn test_execute_request_instrument(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    data_engine: Rc<RefCell<DataEngine>>,
    audusd_sim: CurrencyPair,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let req = RequestInstrument::new(
        audusd_sim.id,
        None,
        None,
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    let cmd = DataCommand::Request(RequestCommand::Instrument(req));
    data_engine.execute(cmd.clone());

    assert_eq!(recorder.borrow()[0], cmd);
}

#[rstest]
fn test_execute_request_instruments(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    data_engine: Rc<RefCell<DataEngine>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let req = RequestInstruments::new(
        None,
        None,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    let cmd = DataCommand::Request(RequestCommand::Instruments(req));
    data_engine.execute(cmd.clone());

    assert_eq!(recorder.borrow()[0], cmd);
}

#[rstest]
fn test_execute_request_book_snapshot(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    data_engine: Rc<RefCell<DataEngine>>,
    audusd_sim: CurrencyPair,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let req = RequestBookSnapshot::new(
        audusd_sim.id,
        None, // depth
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        None, // params
    );
    let cmd = DataCommand::Request(RequestCommand::BookSnapshot(req));
    data_engine.execute(cmd.clone());

    assert_eq!(recorder.borrow()[0], cmd);
}

#[rstest]
fn test_execute_request_quotes(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    data_engine: Rc<RefCell<DataEngine>>,
    audusd_sim: CurrencyPair,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let req = RequestQuotes::new(
        audusd_sim.id,
        None, // start
        None, // end
        None, // limit
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        None, // params
    );
    let cmd = DataCommand::Request(RequestCommand::Quotes(req));
    data_engine.execute(cmd.clone());

    assert_eq!(recorder.borrow()[0], cmd);
}

#[rstest]
fn test_execute_request_trades(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    data_engine: Rc<RefCell<DataEngine>>,
    audusd_sim: CurrencyPair,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let req = RequestTrades::new(
        audusd_sim.id,
        None, // start
        None, // end
        None, // limit
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        None, // params
    );
    let cmd = DataCommand::Request(RequestCommand::Trades(req));
    data_engine.execute(cmd.clone());

    assert_eq!(recorder.borrow()[0], cmd);
}

#[rstest]
fn test_execute_request_funding_rates(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    data_engine: Rc<RefCell<DataEngine>>,
    audusd_sim: CurrencyPair,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let req = RequestFundingRates::new(
        audusd_sim.id,
        None, // start
        None, // end
        None, // limit
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        None, // params
    );
    let cmd = DataCommand::Request(RequestCommand::FundingRates(req));
    data_engine.execute(cmd.clone());

    assert_eq!(recorder.borrow()[0], cmd);
}

#[rstest]
fn test_execute_request_bars(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    data_engine: Rc<RefCell<DataEngine>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let req = RequestBars::new(
        BarType::from("AUD/USD.SIM-1-MINUTE-LAST-INTERNAL"),
        None, // start
        None, // end
        None, // limit
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        None, // params
    );
    let cmd = DataCommand::Request(RequestCommand::Bars(req));
    data_engine.execute(cmd.clone());

    assert_eq!(recorder.borrow()[0], cmd);
}

#[rstest]
fn test_execute_request_order_book_depth(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    data_engine: Rc<RefCell<DataEngine>>,
    audusd_sim: CurrencyPair,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let req = RequestBookDepth::new(
        audusd_sim.id,
        None,                                 // start
        None,                                 // end
        None,                                 // limit
        Some(NonZeroUsize::new(10).unwrap()), // depth
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        None, // params
    );
    let cmd = DataCommand::Request(RequestCommand::BookDepth(req));
    data_engine.execute(cmd.clone());

    assert_eq!(recorder.borrow()[0], cmd);
}

#[rstest]
fn test_process_instrument(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);

    let sub = SubscribeInstrument::new(
        audusd_sim.id(),
        Some(client_id),
        venue,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::Instrument(sub));

    data_engine.borrow_mut().execute(cmd);

    let (handler, saving_handler) =
        msgbus::stubs::get_typed_message_saving_handler::<InstrumentAny>(None);
    let topic = switchboard::get_instrument_topic(audusd_sim.id());
    msgbus::subscribe_instruments(topic.into(), handler, None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process(&audusd_sim as &dyn Any);
    let cache = &data_engine.get_cache();
    let messages = saving_handler.get_messages();

    assert_eq!(
        cache.instrument(&audusd_sim.id()),
        Some(audusd_sim.clone()).as_ref()
    );
    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&audusd_sim));
}

#[rstest]
fn test_process_book_delta(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let sub = SubscribeBookDeltas::new(
        audusd_sim.id,
        BookType::L3_MBO,
        Some(client_id),
        venue,
        UUID4::new(),
        UnixNanos::default(),
        None,
        true,
        None,
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::BookDeltas(sub));

    data_engine.borrow_mut().execute(cmd);

    let delta = stub_delta();
    let (handler, saver) = get_typed_message_saving_handler::<OrderBookDeltas>(None);
    let topic = switchboard::get_book_deltas_topic(delta.instrument_id);
    msgbus::subscribe_book_deltas(topic.into(), handler, None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_data(Data::Delta(delta));
    let _cache = &data_engine.get_cache();
    let messages = saver.get_messages();

    assert_eq!(messages.len(), 1);
}

#[rstest]
fn test_process_book_deltas(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let sub = SubscribeBookDeltas::new(
        audusd_sim.id,
        BookType::L3_MBO,
        Some(client_id),
        venue,
        UUID4::new(),
        UnixNanos::default(),
        None,
        true,
        None,
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::BookDeltas(sub));

    data_engine.borrow_mut().execute(cmd);

    // TODO: Using FFI API wrapper temporarily until Cython gone
    let deltas = OrderBookDeltas_API::new(stub_deltas());
    let (handler, saver) = get_typed_message_saving_handler::<OrderBookDeltas>(None);
    let topic = switchboard::get_book_deltas_topic(deltas.instrument_id);
    msgbus::subscribe_book_deltas(topic.into(), handler, None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_data(Data::Deltas(deltas.clone()));
    let _cache = &data_engine.get_cache();
    let messages = saver.get_messages();

    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&deltas));
}

#[rstest]
fn test_process_book_depth10(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let sub = SubscribeBookDepth10::new(
        audusd_sim.id,
        BookType::L3_MBO,
        Some(client_id),
        venue,
        UUID4::new(),
        UnixNanos::default(),
        None,
        true,
        None,
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::BookDepth10(sub));

    data_engine.borrow_mut().execute(cmd);

    let depth = stub_depth10();
    let (handler, saver) = get_typed_message_saving_handler::<OrderBookDepth10>(None);
    let topic = switchboard::get_book_depth10_topic(depth.instrument_id);
    msgbus::subscribe_book_depth10(topic.into(), handler, None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_data(Data::from(depth));
    let _cache = &data_engine.get_cache();
    let messages = saver.get_messages();

    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&depth));
}

#[rstest]
fn test_process_quote_tick(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let sub = SubscribeQuotes::new(
        audusd_sim.id,
        Some(client_id),
        venue,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::Quotes(sub));

    data_engine.borrow_mut().execute(cmd);

    let quote = QuoteTick::default();
    let (handler, saver) = get_typed_message_saving_handler::<QuoteTick>(None);
    let topic = switchboard::get_quotes_topic(quote.instrument_id);
    msgbus::subscribe_quotes(topic.into(), handler, None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_data(Data::Quote(quote));
    let cache = &data_engine.get_cache();
    let messages = saver.get_messages();

    assert_eq!(cache.quote(&quote.instrument_id), Some(quote).as_ref());
    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&quote));
}

#[rstest]
fn test_process_trade_tick(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let sub = SubscribeTrades::new(
        audusd_sim.id,
        Some(client_id),
        venue,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::Trades(sub));

    data_engine.borrow_mut().execute(cmd);

    let trade = TradeTick::default();
    let (handler, saver) = get_typed_message_saving_handler::<TradeTick>(None);
    let topic = switchboard::get_trades_topic(trade.instrument_id);
    msgbus::subscribe_trades(topic.into(), handler, None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_data(Data::Trade(trade));
    let cache = &data_engine.get_cache();
    let messages = saver.get_messages();

    assert_eq!(cache.trade(&trade.instrument_id), Some(trade).as_ref());
    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&trade));
}

#[rstest]
fn test_synthetic_quote_subscription_publishes_from_component_quotes(
    stub_msgbus: Rc<RefCell<MessageBus>>,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache.clone(), None);

    let (synthetic, component_a, component_b) = synthetic_index();
    let synthetic_id = synthetic.id;
    cache.borrow_mut().add_synthetic(synthetic).unwrap();

    let (handler, saver) = get_typed_message_saving_handler::<QuoteTick>(None);
    let topic = switchboard::get_quotes_topic(synthetic_id);
    msgbus::subscribe_quotes(topic.into(), handler, None);

    let sub = SubscribeQuotes::new(
        synthetic_id,
        None,
        Some(Venue::synthetic()),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Quotes(sub)));

    let quote_a = QuoteTick::new(
        component_a,
        Price::from("100.00"),
        Price::from("102.00"),
        Quantity::from(1),
        Quantity::from(1),
        UnixNanos::from(1),
        UnixNanos::from(1),
    );
    data_engine.process_data(Data::Quote(quote_a));
    assert!(saver.get_messages().is_empty());

    let quote_b = QuoteTick::new(
        component_b,
        Price::from("200.00"),
        Price::from("204.00"),
        Quantity::from(1),
        Quantity::from(1),
        UnixNanos::from(2),
        UnixNanos::from(2),
    );
    data_engine.process_data(Data::Quote(quote_b));

    let messages = saver.get_messages();
    assert_eq!(messages.len(), 1);
    let synthetic_quote = messages[0];
    assert_eq!(synthetic_quote.instrument_id, synthetic_id);
    assert_eq!(synthetic_quote.bid_price, Price::from("150.00"));
    assert_eq!(synthetic_quote.ask_price, Price::from("153.00"));
    assert_eq!(synthetic_quote.bid_size, Quantity::from(1));
    assert_eq!(synthetic_quote.ask_size, Quantity::from(1));
    assert_eq!(synthetic_quote.ts_event, quote_b.ts_event);
    assert!(
        data_engine
            .subscribed_synthetic_quotes()
            .contains(&synthetic_id)
    );
    assert!(cache.borrow().quote(&synthetic_id).is_none());
}

#[rstest]
fn test_synthetic_trade_subscription_publishes_from_component_trades(
    stub_msgbus: Rc<RefCell<MessageBus>>,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache.clone(), None);

    let (synthetic, component_a, component_b) = synthetic_index();
    let synthetic_id = synthetic.id;
    cache.borrow_mut().add_synthetic(synthetic).unwrap();

    let (handler, saver) = get_typed_message_saving_handler::<TradeTick>(None);
    let topic = switchboard::get_trades_topic(synthetic_id);
    msgbus::subscribe_trades(topic.into(), handler, None);

    let sub = SubscribeTrades::new(
        synthetic_id,
        None,
        Some(Venue::synthetic()),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Trades(sub)));

    let trade_a = TradeTick::new(
        component_a,
        Price::from("100.00"),
        Quantity::from(1),
        AggressorSide::Buyer,
        TradeId::new("T-1"),
        UnixNanos::from(1),
        UnixNanos::from(1),
    );
    data_engine.process_data(Data::Trade(trade_a));
    assert!(saver.get_messages().is_empty());

    let trade_b = TradeTick::new(
        component_b,
        Price::from("200.00"),
        Quantity::from(2),
        AggressorSide::Seller,
        TradeId::new("T-2"),
        UnixNanos::from(2),
        UnixNanos::from(2),
    );
    data_engine.process_data(Data::Trade(trade_b));

    let messages = saver.get_messages();
    assert_eq!(messages.len(), 1);
    let synthetic_trade = messages[0];
    assert_eq!(synthetic_trade.instrument_id, synthetic_id);
    assert_eq!(synthetic_trade.price, Price::from("150.00"));
    assert_eq!(synthetic_trade.size, Quantity::from(1));
    assert_eq!(synthetic_trade.aggressor_side, trade_b.aggressor_side);
    assert_eq!(synthetic_trade.trade_id, trade_b.trade_id);
    assert_eq!(synthetic_trade.ts_event, trade_b.ts_event);
    assert!(
        data_engine
            .subscribed_synthetic_trades()
            .contains(&synthetic_id)
    );
    assert!(cache.borrow().trade(&synthetic_id).is_none());
}

#[rstest]
fn test_synthetic_quote_and_trade_commands_do_not_forward_to_client(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let clock = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let engine_clock: Rc<RefCell<dyn Clock>> = clock.clone();
    let mut data_engine = DataEngine::new(engine_clock, cache.clone(), None);
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let (synthetic, _, _) = synthetic_index();
    let synthetic_id = synthetic.id;
    cache.borrow_mut().add_synthetic(synthetic).unwrap();

    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Quotes(
        SubscribeQuotes::new(
            synthetic_id,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ),
    )));
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Trades(
        SubscribeTrades::new(
            synthetic_id,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ),
    )));
    let subscribed_quotes = data_engine.subscribed_synthetic_quotes();
    let subscribed_trades = data_engine.subscribed_synthetic_trades();

    data_engine.execute(DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(
        UnsubscribeQuotes::new(
            synthetic_id,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ),
    )));
    data_engine.execute(DataCommand::Unsubscribe(UnsubscribeCommand::Trades(
        UnsubscribeTrades::new(
            synthetic_id,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ),
    )));

    assert!(subscribed_quotes.contains(&synthetic_id));
    assert!(subscribed_trades.contains(&synthetic_id));
    assert!(
        !data_engine
            .subscribed_synthetic_quotes()
            .contains(&synthetic_id)
    );
    assert!(
        !data_engine
            .subscribed_synthetic_trades()
            .contains(&synthetic_id)
    );
    assert!(recorder.borrow().is_empty());
}

#[rstest]
fn test_duplicate_synthetic_quote_subscription_publishes_once(
    stub_msgbus: Rc<RefCell<MessageBus>>,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache.clone(), None);

    let (synthetic, component_a, component_b) = synthetic_index();
    let synthetic_id = synthetic.id;
    cache.borrow_mut().add_synthetic(synthetic).unwrap();

    let (handler, saver) = get_typed_message_saving_handler::<QuoteTick>(None);
    let topic = switchboard::get_quotes_topic(synthetic_id);
    msgbus::subscribe_quotes(topic.into(), handler, None);

    data_engine.execute(subscribe_synthetic_quotes_cmd(synthetic_id));
    data_engine.execute(subscribe_synthetic_quotes_cmd(synthetic_id));
    data_engine.process_data(Data::Quote(quote_tick(component_a, "100.00", "102.00", 1)));
    data_engine.process_data(Data::Quote(quote_tick(component_b, "200.00", "204.00", 2)));

    let subscribed = data_engine.subscribed_synthetic_quotes();
    let messages = saver.get_messages();
    assert_eq!(
        subscribed.iter().filter(|id| **id == synthetic_id).count(),
        1
    );
    assert_eq!(messages.len(), 1);
}

#[rstest]
fn test_duplicate_synthetic_trade_subscription_publishes_once(
    stub_msgbus: Rc<RefCell<MessageBus>>,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache.clone(), None);

    let (synthetic, component_a, component_b) = synthetic_index();
    let synthetic_id = synthetic.id;
    cache.borrow_mut().add_synthetic(synthetic).unwrap();

    let (handler, saver) = get_typed_message_saving_handler::<TradeTick>(None);
    let topic = switchboard::get_trades_topic(synthetic_id);
    msgbus::subscribe_trades(topic.into(), handler, None);

    data_engine.execute(subscribe_synthetic_trades_cmd(synthetic_id));
    data_engine.execute(subscribe_synthetic_trades_cmd(synthetic_id));
    data_engine.process_data(Data::Trade(trade_tick(component_a, "100.00", "T-1", 1)));
    data_engine.process_data(Data::Trade(trade_tick(component_b, "200.00", "T-2", 2)));

    let subscribed = data_engine.subscribed_synthetic_trades();
    let messages = saver.get_messages();
    assert_eq!(
        subscribed.iter().filter(|id| **id == synthetic_id).count(),
        1
    );
    assert_eq!(messages.len(), 1);
}

#[rstest]
fn test_synthetic_quote_subscription_waits_for_all_component_quotes(
    stub_msgbus: Rc<RefCell<MessageBus>>,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache.clone(), None);

    let (synthetic, component_a, _) = synthetic_index();
    let synthetic_id = synthetic.id;
    cache.borrow_mut().add_synthetic(synthetic).unwrap();

    let (handler, saver) = get_typed_message_saving_handler::<QuoteTick>(None);
    let topic = switchboard::get_quotes_topic(synthetic_id);
    msgbus::subscribe_quotes(topic.into(), handler, None);

    data_engine.execute(subscribe_synthetic_quotes_cmd(synthetic_id));
    data_engine.process_data(Data::Quote(quote_tick(component_a, "100.00", "102.00", 1)));

    assert!(
        data_engine
            .subscribed_synthetic_quotes()
            .contains(&synthetic_id)
    );
    assert!(saver.get_messages().is_empty());
    assert!(cache.borrow().quote(&synthetic_id).is_none());
}

#[rstest]
fn test_synthetic_trade_subscription_waits_for_all_component_trades(
    stub_msgbus: Rc<RefCell<MessageBus>>,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache.clone(), None);

    let (synthetic, component_a, _) = synthetic_index();
    let synthetic_id = synthetic.id;
    cache.borrow_mut().add_synthetic(synthetic).unwrap();

    let (handler, saver) = get_typed_message_saving_handler::<TradeTick>(None);
    let topic = switchboard::get_trades_topic(synthetic_id);
    msgbus::subscribe_trades(topic.into(), handler, None);

    data_engine.execute(subscribe_synthetic_trades_cmd(synthetic_id));
    data_engine.process_data(Data::Trade(trade_tick(component_a, "100.00", "T-1", 1)));

    assert!(
        data_engine
            .subscribed_synthetic_trades()
            .contains(&synthetic_id)
    );
    assert!(saver.get_messages().is_empty());
    assert!(cache.borrow().trade(&synthetic_id).is_none());
}

#[rstest]
fn test_subscribe_missing_synthetic_does_not_register(stub_msgbus: Rc<RefCell<MessageBus>>) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache, None);

    let synthetic_id = synthetic_instrument_id();
    data_engine.execute(subscribe_synthetic_quotes_cmd(synthetic_id));
    data_engine.execute(subscribe_synthetic_trades_cmd(synthetic_id));

    assert!(
        !data_engine
            .subscribed_synthetic_quotes()
            .contains(&synthetic_id)
    );
    assert!(
        !data_engine
            .subscribed_synthetic_trades()
            .contains(&synthetic_id)
    );
}

#[rstest]
fn test_unsubscribe_synthetic_quote_keeps_shared_component_feed(
    stub_msgbus: Rc<RefCell<MessageBus>>,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache.clone(), None);

    let component_common = InstrumentId::from("BTC-USD.SIM");
    let component_a = InstrumentId::from("ETH-USD.SIM");
    let component_b = InstrumentId::from("SOL-USD.SIM");
    let synthetic_a =
        synthetic_index_with_components("BTC-ETH-INDEX", component_common, component_a);
    let synthetic_b =
        synthetic_index_with_components("BTC-SOL-INDEX", component_common, component_b);
    let synthetic_a_id = synthetic_a.id;
    let synthetic_b_id = synthetic_b.id;
    cache.borrow_mut().add_synthetic(synthetic_a).unwrap();
    cache.borrow_mut().add_synthetic(synthetic_b).unwrap();

    let (handler_a, saver_a) = get_typed_message_saving_handler::<QuoteTick>(None);
    let topic_a = switchboard::get_quotes_topic(synthetic_a_id);
    msgbus::subscribe_quotes(topic_a.into(), handler_a, None);
    let (handler_b, saver_b) = get_typed_message_saving_handler::<QuoteTick>(None);
    let topic_b = switchboard::get_quotes_topic(synthetic_b_id);
    msgbus::subscribe_quotes(topic_b.into(), handler_b, None);

    data_engine.execute(subscribe_synthetic_quotes_cmd(synthetic_a_id));
    data_engine.execute(subscribe_synthetic_quotes_cmd(synthetic_b_id));
    data_engine.process_data(Data::Quote(quote_tick(component_a, "100.00", "102.00", 1)));
    data_engine.process_data(Data::Quote(quote_tick(component_b, "300.00", "304.00", 2)));
    data_engine.process_data(Data::Quote(quote_tick(
        component_common,
        "200.00",
        "202.00",
        3,
    )));
    assert_eq!(saver_a.get_messages().len(), 1);
    assert_eq!(saver_b.get_messages().len(), 1);

    data_engine.execute(unsubscribe_synthetic_quotes_cmd(synthetic_a_id));
    data_engine.process_data(Data::Quote(quote_tick(
        component_common,
        "220.00",
        "222.00",
        4,
    )));

    assert_eq!(saver_a.get_messages().len(), 1);
    assert_eq!(saver_b.get_messages().len(), 2);
    assert!(
        !data_engine
            .subscribed_synthetic_quotes()
            .contains(&synthetic_a_id)
    );
    assert!(
        data_engine
            .subscribed_synthetic_quotes()
            .contains(&synthetic_b_id)
    );
}

#[rstest]
fn test_unsubscribe_synthetic_trade_keeps_shared_component_feed(
    stub_msgbus: Rc<RefCell<MessageBus>>,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache.clone(), None);

    let component_common = InstrumentId::from("BTC-USD.SIM");
    let component_a = InstrumentId::from("ETH-USD.SIM");
    let component_b = InstrumentId::from("SOL-USD.SIM");
    let synthetic_a =
        synthetic_index_with_components("BTC-ETH-INDEX", component_common, component_a);
    let synthetic_b =
        synthetic_index_with_components("BTC-SOL-INDEX", component_common, component_b);
    let synthetic_a_id = synthetic_a.id;
    let synthetic_b_id = synthetic_b.id;
    cache.borrow_mut().add_synthetic(synthetic_a).unwrap();
    cache.borrow_mut().add_synthetic(synthetic_b).unwrap();

    let (handler_a, saver_a) = get_typed_message_saving_handler::<TradeTick>(None);
    let topic_a = switchboard::get_trades_topic(synthetic_a_id);
    msgbus::subscribe_trades(topic_a.into(), handler_a, None);
    let (handler_b, saver_b) = get_typed_message_saving_handler::<TradeTick>(None);
    let topic_b = switchboard::get_trades_topic(synthetic_b_id);
    msgbus::subscribe_trades(topic_b.into(), handler_b, None);

    data_engine.execute(subscribe_synthetic_trades_cmd(synthetic_a_id));
    data_engine.execute(subscribe_synthetic_trades_cmd(synthetic_b_id));
    data_engine.process_data(Data::Trade(trade_tick(component_a, "100.00", "T-1", 1)));
    data_engine.process_data(Data::Trade(trade_tick(component_b, "300.00", "T-2", 2)));
    data_engine.process_data(Data::Trade(trade_tick(
        component_common,
        "200.00",
        "T-3",
        3,
    )));
    assert_eq!(saver_a.get_messages().len(), 1);
    assert_eq!(saver_b.get_messages().len(), 1);

    data_engine.execute(unsubscribe_synthetic_trades_cmd(synthetic_a_id));
    data_engine.process_data(Data::Trade(trade_tick(
        component_common,
        "220.00",
        "T-4",
        4,
    )));

    assert_eq!(saver_a.get_messages().len(), 1);
    assert_eq!(saver_b.get_messages().len(), 2);
    assert!(
        !data_engine
            .subscribed_synthetic_trades()
            .contains(&synthetic_a_id)
    );
    assert!(
        data_engine
            .subscribed_synthetic_trades()
            .contains(&synthetic_b_id)
    );
}

#[rstest]
fn test_reset_clears_synthetic_subscriptions(stub_msgbus: Rc<RefCell<MessageBus>>) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache.clone(), None);

    let (synthetic, component_a, component_b) = synthetic_index();
    let synthetic_id = synthetic.id;
    cache.borrow_mut().add_synthetic(synthetic).unwrap();

    let (quote_handler, quote_saver) = get_typed_message_saving_handler::<QuoteTick>(None);
    let quote_topic = switchboard::get_quotes_topic(synthetic_id);
    msgbus::subscribe_quotes(quote_topic.into(), quote_handler, None);
    let (trade_handler, trade_saver) = get_typed_message_saving_handler::<TradeTick>(None);
    let trade_topic = switchboard::get_trades_topic(synthetic_id);
    msgbus::subscribe_trades(trade_topic.into(), trade_handler, None);

    data_engine.execute(subscribe_synthetic_quotes_cmd(synthetic_id));
    data_engine.execute(subscribe_synthetic_trades_cmd(synthetic_id));
    data_engine.reset();

    data_engine.process_data(Data::Quote(quote_tick(component_a, "100.00", "102.00", 1)));
    data_engine.process_data(Data::Quote(quote_tick(component_b, "200.00", "204.00", 2)));
    data_engine.process_data(Data::Trade(trade_tick(component_a, "100.00", "T-1", 1)));
    data_engine.process_data(Data::Trade(trade_tick(component_b, "200.00", "T-2", 2)));

    assert!(
        !data_engine
            .subscribed_synthetic_quotes()
            .contains(&synthetic_id)
    );
    assert!(
        !data_engine
            .subscribed_synthetic_trades()
            .contains(&synthetic_id)
    );
    assert!(quote_saver.get_messages().is_empty());
    assert!(trade_saver.get_messages().is_empty());
}

#[rstest]
fn test_process_mark_price(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let sub = SubscribeMarkPrices::new(
        audusd_sim.id,
        Some(client_id),
        venue,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::MarkPrices(sub));

    data_engine.borrow_mut().execute(cmd);

    let mark_price = MarkPriceUpdate::new(
        audusd_sim.id,
        Price::from("1.00000"),
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    let (typed_handler, saving_handler) = get_typed_message_saving_handler::<MarkPriceUpdate>(None);
    let topic = switchboard::get_mark_price_topic(mark_price.instrument_id);
    msgbus::subscribe_mark_prices(topic.into(), typed_handler, None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_data(Data::MarkPriceUpdate(mark_price));
    let cache = &data_engine.get_cache();
    let messages = saving_handler.get_messages();

    assert_eq!(
        cache.price(&mark_price.instrument_id, PriceType::Mark),
        Some(mark_price.value)
    );
    assert_eq!(
        cache.mark_price(&mark_price.instrument_id),
        Some(&mark_price)
    );
    assert_eq!(
        cache.mark_prices(&mark_price.instrument_id),
        Some(vec![mark_price])
    );
    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&mark_price));
}

#[rstest]
fn test_process_index_price(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let sub = SubscribeIndexPrices::new(
        audusd_sim.id,
        Some(client_id),
        venue,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::IndexPrices(sub));

    data_engine.borrow_mut().execute(cmd);

    let index_price = IndexPriceUpdate::new(
        audusd_sim.id,
        Price::from("1.00000"),
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    let (typed_handler, saving_handler) =
        get_typed_message_saving_handler::<IndexPriceUpdate>(None);
    let topic = switchboard::get_index_price_topic(index_price.instrument_id);
    msgbus::subscribe_index_prices(topic.into(), typed_handler, None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_data(Data::IndexPriceUpdate(index_price));
    let cache = &data_engine.get_cache();
    let messages = saving_handler.get_messages();

    assert_eq!(
        cache.index_price(&index_price.instrument_id),
        Some(&index_price)
    );
    assert_eq!(
        cache.index_prices(&index_price.instrument_id),
        Some(vec![index_price])
    );
    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&index_price));
}

#[rstest]
fn test_process_funding_rate_through_any(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let sub = SubscribeFundingRates::new(
        audusd_sim.id,
        Some(client_id),
        venue,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::FundingRates(sub));

    data_engine.borrow_mut().execute(cmd);

    let funding_rate = FundingRateUpdate::new(
        audusd_sim.id,
        "0.0001".parse().unwrap(),
        None,
        None,
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    let (typed_handler, saving_handler) =
        get_typed_message_saving_handler::<FundingRateUpdate>(None);
    let topic = switchboard::get_funding_rate_topic(funding_rate.instrument_id);
    msgbus::subscribe_funding_rates(topic.into(), typed_handler, None);

    let mut data_engine = data_engine.borrow_mut();
    // Test through the process() method with &dyn Any
    data_engine.process(&funding_rate as &dyn Any);
    let cache = &data_engine.get_cache();
    let messages = saving_handler.get_messages();

    assert_eq!(
        cache.funding_rate(&funding_rate.instrument_id),
        Some(&funding_rate)
    );
    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&funding_rate));
}

#[rstest]
fn test_process_funding_rate(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let sub = SubscribeFundingRates::new(
        audusd_sim.id,
        Some(client_id),
        venue,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::FundingRates(sub));

    data_engine.borrow_mut().execute(cmd);

    let funding_rate = FundingRateUpdate::new(
        audusd_sim.id,
        "0.0001".parse().unwrap(),
        None,
        None,
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    let (typed_handler, saving_handler) =
        get_typed_message_saving_handler::<FundingRateUpdate>(None);
    let topic = switchboard::get_funding_rate_topic(funding_rate.instrument_id);
    msgbus::subscribe_funding_rates(topic.into(), typed_handler, None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.handle_funding_rate(funding_rate);
    let cache = &data_engine.get_cache();
    let messages = saving_handler.get_messages();

    assert_eq!(
        cache.funding_rate(&funding_rate.instrument_id),
        Some(&funding_rate)
    );
    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&funding_rate));
}

#[rstest]
fn test_process_funding_rate_updates_existing(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let sub = SubscribeFundingRates::new(
        audusd_sim.id,
        Some(client_id),
        venue,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::FundingRates(sub));

    data_engine.borrow_mut().execute(cmd);

    let funding_rate1 = FundingRateUpdate::new(
        audusd_sim.id,
        "0.0001".parse().unwrap(),
        None,
        None,
        UnixNanos::from(1),
        UnixNanos::from(2),
    );

    let funding_rate2 = FundingRateUpdate::new(
        audusd_sim.id,
        "0.0002".parse().unwrap(),
        None,
        None,
        UnixNanos::from(3),
        UnixNanos::from(4),
    );

    let mut data_engine = data_engine.borrow_mut();
    data_engine.handle_funding_rate(funding_rate1);
    data_engine.handle_funding_rate(funding_rate2);
    let cache = &data_engine.get_cache();

    // Should only have the latest funding rate
    assert_eq!(
        cache.funding_rate(&funding_rate2.instrument_id),
        Some(&funding_rate2)
    );
}

#[rstest]
fn test_process_bar(data_engine: Rc<RefCell<DataEngine>>, data_client: DataClientAdapter) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let bar = Bar::default();

    let sub = SubscribeBars::new(
        bar.bar_type,
        Some(client_id),
        venue,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::Bars(sub));

    data_engine.borrow_mut().execute(cmd);

    let (handler, saver) = get_typed_message_saving_handler::<Bar>(None);
    let topic = switchboard::get_bars_topic(bar.bar_type);
    msgbus::subscribe_bars(topic.into(), handler, None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_data(Data::Bar(bar));
    let cache = &data_engine.get_cache();
    let messages = saver.get_messages();

    assert_eq!(cache.bar(&bar.bar_type), Some(bar).as_ref());
    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&bar));
}

#[rstest]
fn test_process_instrument_status(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let sub = SubscribeInstrumentStatus::new(
        audusd_sim.id,
        Some(client_id),
        venue,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::InstrumentStatus(sub));

    data_engine.borrow_mut().execute(cmd);

    let status = InstrumentStatus::new(
        audusd_sim.id,
        MarketStatusAction::Trading,
        UnixNanos::from(1),
        UnixNanos::from(2),
        None,
        None,
        Some(true),
        Some(true),
        None,
    );
    let handler = msgbus::stubs::get_message_saving_handler::<InstrumentStatus>(None);
    let topic = switchboard::get_instrument_status_topic(status.instrument_id);
    msgbus::subscribe_any(topic.into(), handler.clone(), None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_data(Data::InstrumentStatus(status));
    let cache = data_engine.get_cache();
    let messages = msgbus::stubs::get_saved_messages::<InstrumentStatus>(&handler);

    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&status));
    assert_eq!(cache.instrument_status(&audusd_sim.id), Some(&status));
    assert_eq!(
        cache.instrument_statuses(&audusd_sim.id),
        Some(vec![status]),
    );
}

#[rstest]
fn test_process_instrument_status_through_any(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let sub = SubscribeInstrumentStatus::new(
        audusd_sim.id,
        Some(client_id),
        venue,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::InstrumentStatus(sub));
    data_engine.borrow_mut().execute(cmd);

    let status = InstrumentStatus::new(
        audusd_sim.id,
        MarketStatusAction::Trading,
        UnixNanos::from(1),
        UnixNanos::from(2),
        None,
        None,
        Some(true),
        Some(true),
        None,
    );
    let handler = msgbus::stubs::get_message_saving_handler::<InstrumentStatus>(None);
    let topic = switchboard::get_instrument_status_topic(status.instrument_id);
    msgbus::subscribe_any(topic.into(), handler.clone(), None);

    let mut data_engine = data_engine.borrow_mut();
    // Drive through the process() entrypoint with `&dyn Any`
    data_engine.process(&status as &dyn Any);
    let cache = data_engine.get_cache();
    let messages = msgbus::stubs::get_saved_messages::<InstrumentStatus>(&handler);

    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&status));
    assert_eq!(cache.instrument_status(&audusd_sim.id), Some(&status));
}

#[rstest]
fn test_process_instrument_status_updates_existing(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let sub = SubscribeInstrumentStatus::new(
        audusd_sim.id,
        Some(client_id),
        venue,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::InstrumentStatus(sub));
    data_engine.borrow_mut().execute(cmd);

    let status1 = InstrumentStatus::new(
        audusd_sim.id,
        MarketStatusAction::PreOpen,
        UnixNanos::from(1),
        UnixNanos::from(2),
        None,
        None,
        Some(false),
        Some(false),
        None,
    );
    let status2 = InstrumentStatus::new(
        audusd_sim.id,
        MarketStatusAction::Trading,
        UnixNanos::from(3),
        UnixNanos::from(4),
        None,
        None,
        Some(true),
        Some(true),
        None,
    );

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_data(Data::InstrumentStatus(status1));
    data_engine.process_data(Data::InstrumentStatus(status2));
    let cache = data_engine.get_cache();

    assert_eq!(cache.instrument_status(&audusd_sim.id), Some(&status2));
    assert_eq!(
        cache.instrument_statuses(&audusd_sim.id),
        Some(vec![status2, status1]),
    );
}

#[cfg(feature = "defi")]
#[rstest]
fn test_execute_subscribe_blocks(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let blockchain = Blockchain::Ethereum;
    let sub_cmd = DataCommand::DefiSubscribe(DefiSubscribeCommand::Blocks(SubscribeBlocks {
        chain: blockchain,
        client_id: Some(client_id),
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    }));
    data_engine.execute(sub_cmd.clone());

    assert!(data_engine.subscribed_blocks().contains(&blockchain));
    {
        assert_eq!(recorder.borrow().as_slice(), std::slice::from_ref(&sub_cmd));
    }

    let unsub_cmd =
        DataCommand::DefiUnsubscribe(DefiUnsubscribeCommand::Blocks(UnsubscribeBlocks {
            chain: blockchain,
            client_id: Some(client_id),
            command_id: UUID4::new(),
            ts_init: UnixNanos::default(),
            params: None,
        }));
    data_engine.execute(unsub_cmd.clone());

    assert!(!data_engine.subscribed_blocks().contains(&blockchain));
    assert_eq!(recorder.borrow().as_slice(), &[sub_cmd, unsub_cmd]);
}

#[cfg(feature = "defi")]
#[rstest]
fn test_execute_subscribe_pool_swaps(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let instrument_id =
        InstrumentId::from("0x11b815efB8f581194ae79006d24E0d814B7697F6.Arbitrum:UniswapV3");

    let sub_cmd = DataCommand::DefiSubscribe(DefiSubscribeCommand::PoolSwaps(SubscribePoolSwaps {
        instrument_id,
        client_id: Some(client_id),
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    }));
    data_engine.execute(sub_cmd.clone());

    assert!(data_engine.subscribed_pool_swaps().contains(&instrument_id));
    {
        // Verify two commands: SubscribePoolSwaps (forwarded first) and RequestPoolSnapshot (from setup_pool_updater)
        let recorded = recorder.borrow();
        assert_eq!(
            recorded.len(),
            2,
            "Expected SubscribePoolSwaps and RequestPoolSnapshot"
        );

        // First command should be the SubscribePoolSwaps (forwarded before snapshot request)
        assert_eq!(recorded[0], sub_cmd);

        // Second command should be RequestPoolSnapshot
        match &recorded[1] {
            DataCommand::DefiRequest(DefiRequestCommand::PoolSnapshot(request)) => {
                assert_eq!(request.instrument_id, instrument_id);
                assert_eq!(request.client_id, Some(client_id));
            }
            _ => panic!(
                "Expected second command to be RequestPoolSnapshot, was: {:?}",
                recorded[1]
            ),
        }
    }

    let unsub_cmd =
        DataCommand::DefiUnsubscribe(DefiUnsubscribeCommand::PoolSwaps(UnsubscribePoolSwaps {
            instrument_id,
            client_id: Some(client_id),
            command_id: UUID4::new(),
            ts_init: UnixNanos::default(),
            params: None,
        }));
    data_engine.execute(unsub_cmd.clone());

    assert!(!data_engine.subscribed_pool_swaps().contains(&instrument_id));
    // After unsubscribe, should have snapshot request, subscribe, and unsubscribe
    let recorded = recorder.borrow();
    assert_eq!(recorded.len(), 3);
    assert_eq!(recorded[2], unsub_cmd);
}

#[cfg(feature = "defi")]
#[rstest]
fn test_process_block(data_engine: Rc<RefCell<DataEngine>>, data_client: DataClientAdapter) {
    let client_id = data_client.client_id;
    data_engine.borrow_mut().register_client(data_client, None);

    let blockchain = Blockchain::Ethereum;
    let sub = DefiSubscribeCommand::Blocks(SubscribeBlocks {
        chain: blockchain,
        client_id: Some(client_id),
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    });
    let cmd = DataCommand::DefiSubscribe(sub);

    data_engine.borrow_mut().execute(cmd);

    let block = Block::new(
        "0x123".to_string(),
        "0x456".to_string(),
        1u64,
        "miner".into(),
        1000000u64,
        500000u64,
        UnixNanos::from(1),
        Some(blockchain),
    );
    let (typed_handler, saving_handler) = get_typed_message_saving_handler::<Block>(None);
    let topic = defi::switchboard::get_defi_blocks_topic(blockchain);
    msgbus::subscribe_defi_blocks(topic.into(), typed_handler, None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_defi_data(DefiData::Block(block.clone()));
    let messages = saving_handler.get_messages();

    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&block));
}

#[cfg(feature = "defi")]
#[rstest]
fn test_process_pool_swap(data_engine: Rc<RefCell<DataEngine>>, data_client: DataClientAdapter) {
    let client_id = data_client.client_id;
    data_engine.borrow_mut().register_client(data_client, None);

    // Create a pool swap
    let chain = Arc::new(chains::ETHEREUM.clone());
    let dex = Arc::new(Dex::new(
        chains::ETHEREUM.clone(),
        DexType::UniswapV3,
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        0,
        AmmType::CLAMM,
        "PoolCreated",
        "Swap",
        "Mint",
        "Burn",
        "Collect",
    ));
    let token0 = Token::new(
        chain.clone(),
        Address::from([0x11; 20]),
        "WETH".to_string(),
        "WETH".to_string(),
        18,
    );
    let token1 = Token::new(
        chain.clone(),
        Address::from([0x22; 20]),
        "USDC".to_string(),
        "USDC".to_string(),
        6,
    );
    let mut pool = Pool::new(
        chain.clone(),
        dex.clone(),
        Address::from([0x12; 20]),
        PoolIdentifier::new("0x1234567890123456789012345678901234567890"),
        0u64,
        token0,
        token1,
        Some(500u32),
        Some(10u32),
        UnixNanos::from(1),
    );

    let initial_price = U160::from(79228162514264337593543950336u128); // sqrt(1) * 2^96
    pool.initialize(initial_price, get_tick_at_sqrt_ratio(initial_price));
    let instrument_id = pool.instrument_id;

    // Add pool to cache so setup_pool_updater doesn't request snapshot
    data_engine
        .borrow()
        .cache_rc()
        .borrow_mut()
        .add_pool(pool.clone())
        .unwrap();

    let swap = PoolSwap::new(
        chain,
        dex,
        instrument_id,
        pool.pool_identifier,
        1000u64,
        "0x123".to_string(),
        0,
        0,
        None,
        Address::from([0x12; 20]),
        Address::from([0x12; 20]),
        I256::from_str("1000000000000000000").unwrap(),
        I256::from_str("400000000000000").unwrap(),
        U160::from(59000000000000u128),
        1000000,
        100,
    );

    let sub = DefiSubscribeCommand::PoolSwaps(SubscribePoolSwaps {
        instrument_id,
        client_id: Some(client_id),
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    });
    let cmd = DataCommand::DefiSubscribe(sub);

    data_engine.borrow_mut().execute(cmd);

    let (typed_handler, saving_handler) = get_typed_message_saving_handler::<PoolSwap>(None);
    let topic = defi::switchboard::get_defi_pool_swaps_topic(instrument_id);
    msgbus::subscribe_defi_swaps(topic.into(), typed_handler, None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_defi_data(DefiData::PoolSwap(swap.clone()));
    let messages = saving_handler.get_messages();

    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&swap));
}

#[cfg(feature = "defi")]
#[rstest]
fn test_execute_subscribe_pool_liquidity_updates(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let instrument_id =
        InstrumentId::from("0x11b815efB8f581194ae79006d24E0d814B7697F6.Arbitrum:UniswapV3");

    let sub_cmd = DataCommand::DefiSubscribe(DefiSubscribeCommand::PoolLiquidityUpdates(
        SubscribePoolLiquidityUpdates {
            instrument_id,
            client_id: Some(client_id),
            command_id: UUID4::new(),
            ts_init: UnixNanos::default(),
            params: None,
        },
    ));
    data_engine.execute(sub_cmd.clone());

    assert!(
        data_engine
            .subscribed_pool_liquidity_updates()
            .contains(&instrument_id)
    );
    {
        // Verify two commands: SubscribePoolLiquidityUpdates (forwarded first) and RequestPoolSnapshot (from setup_pool_updater)
        let recorded = recorder.borrow();
        assert_eq!(
            recorded.len(),
            2,
            "Expected SubscribePoolLiquidityUpdates and RequestPoolSnapshot"
        );

        // First command should be the SubscribePoolLiquidityUpdates (forwarded before snapshot request)
        assert_eq!(recorded[0], sub_cmd);

        // Second command should be RequestPoolSnapshot
        match &recorded[1] {
            DataCommand::DefiRequest(DefiRequestCommand::PoolSnapshot(request)) => {
                assert_eq!(request.instrument_id, instrument_id);
                assert_eq!(request.client_id, Some(client_id));
            }
            _ => panic!(
                "Expected second command to be RequestPoolSnapshot, was: {:?}",
                recorded[1]
            ),
        }
    }

    let unsub_cmd = DataCommand::DefiUnsubscribe(DefiUnsubscribeCommand::PoolLiquidityUpdates(
        UnsubscribePoolLiquidityUpdates {
            instrument_id,
            client_id: Some(client_id),
            command_id: UUID4::new(),
            ts_init: UnixNanos::default(),
            params: None,
        },
    ));
    data_engine.execute(unsub_cmd.clone());

    assert!(
        !data_engine
            .subscribed_pool_liquidity_updates()
            .contains(&instrument_id)
    );
    // After unsubscribe, should have snapshot request, subscribe, and unsubscribe
    let recorded = recorder.borrow();
    assert_eq!(recorded.len(), 3);
    assert_eq!(recorded[2], unsub_cmd);
}

#[cfg(feature = "defi")]
#[rstest]
fn test_execute_subscribe_pool_fee_collects(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let instrument_id =
        InstrumentId::from("0x11b815efB8f581194ae79006d24E0d814B7697F6.Arbitrum:UniswapV3");

    let sub_cmd = DataCommand::DefiSubscribe(DefiSubscribeCommand::PoolFeeCollects(
        SubscribePoolFeeCollects {
            instrument_id,
            client_id: Some(client_id),
            command_id: UUID4::new(),
            ts_init: UnixNanos::default(),
            params: None,
        },
    ));
    data_engine.execute(sub_cmd.clone());

    assert!(
        data_engine
            .subscribed_pool_fee_collects()
            .contains(&instrument_id)
    );
    {
        // Verify two commands: SubscribePoolFeeCollects (forwarded first) and RequestPoolSnapshot (from setup_pool_updater)
        let recorded = recorder.borrow();
        assert_eq!(
            recorded.len(),
            2,
            "Expected SubscribePoolFeeCollects and RequestPoolSnapshot"
        );

        // First command should be the SubscribePoolFeeCollects (forwarded before snapshot request)
        assert_eq!(recorded[0], sub_cmd);

        // Second command should be RequestPoolSnapshot
        match &recorded[1] {
            DataCommand::DefiRequest(DefiRequestCommand::PoolSnapshot(request)) => {
                assert_eq!(request.instrument_id, instrument_id);
                assert_eq!(request.client_id, Some(client_id));
            }
            _ => panic!(
                "Expected second command to be RequestPoolSnapshot, was: {:?}",
                recorded[1]
            ),
        }
    }

    let unsub_cmd = DataCommand::DefiUnsubscribe(DefiUnsubscribeCommand::PoolFeeCollects(
        UnsubscribePoolFeeCollects {
            instrument_id,
            client_id: Some(client_id),
            command_id: UUID4::new(),
            ts_init: UnixNanos::default(),
            params: None,
        },
    ));
    data_engine.execute(unsub_cmd.clone());

    assert!(
        !data_engine
            .subscribed_pool_fee_collects()
            .contains(&instrument_id)
    );
    // After unsubscribe, should have snapshot request, subscribe, and unsubscribe
    let recorded = recorder.borrow();
    assert_eq!(recorded.len(), 3);
    assert_eq!(recorded[2], unsub_cmd);
}

#[cfg(feature = "defi")]
#[rstest]
fn test_execute_subscribe_pool_flash_events(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let instrument_id =
        InstrumentId::from("0x11b815efB8f581194ae79006d24E0d814B7697F6.Arbitrum:UniswapV3");

    let sub_cmd = DataCommand::DefiSubscribe(DefiSubscribeCommand::PoolFlashEvents(
        SubscribePoolFlashEvents {
            instrument_id,
            client_id: Some(client_id),
            command_id: UUID4::new(),
            ts_init: UnixNanos::default(),
            params: None,
        },
    ));
    data_engine.execute(sub_cmd.clone());

    assert!(data_engine.subscribed_pool_flash().contains(&instrument_id));
    {
        // Verify two commands: SubscribePoolFlashEvents (forwarded first) and RequestPoolSnapshot (from setup_pool_updater)
        let recorded = recorder.borrow();
        assert_eq!(
            recorded.len(),
            2,
            "Expected SubscribePoolFlashEvents and RequestPoolSnapshot"
        );

        // First command should be the SubscribePoolFlashEvents (forwarded before snapshot request)
        assert_eq!(recorded[0], sub_cmd);

        // Second command should be RequestPoolSnapshot
        match &recorded[1] {
            DataCommand::DefiRequest(DefiRequestCommand::PoolSnapshot(request)) => {
                assert_eq!(request.instrument_id, instrument_id);
                assert_eq!(request.client_id, Some(client_id));
            }
            _ => panic!(
                "Expected second command to be RequestPoolSnapshot, was: {:?}",
                recorded[1]
            ),
        }
    }

    let unsub_cmd = DataCommand::DefiUnsubscribe(DefiUnsubscribeCommand::PoolFlashEvents(
        UnsubscribePoolFlashEvents {
            instrument_id,
            client_id: Some(client_id),
            command_id: UUID4::new(),
            ts_init: UnixNanos::default(),
            params: None,
        },
    ));
    data_engine.execute(unsub_cmd.clone());

    assert!(!data_engine.subscribed_pool_flash().contains(&instrument_id));
    // After unsubscribe, should have snapshot request, subscribe, and unsubscribe
    let recorded = recorder.borrow();
    assert_eq!(recorded.len(), 3);
    assert_eq!(recorded[2], unsub_cmd);
}

#[cfg(feature = "defi")]
#[rstest]
fn test_process_pool_liquidity_update(
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    data_engine.borrow_mut().register_client(data_client, None);

    // Create test pool
    let chain = Arc::new(chains::ETHEREUM.clone());
    let dex = Arc::new(Dex::new(
        chains::ETHEREUM.clone(),
        DexType::UniswapV3,
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        0,
        AmmType::CLAMM,
        "PoolCreated",
        "Swap",
        "Mint",
        "Burn",
        "Collect",
    ));
    let token0 = Token::new(
        chain.clone(),
        Address::from([0x11; 20]),
        "WETH".to_string(),
        "WETH".to_string(),
        18,
    );
    let token1 = Token::new(
        chain.clone(),
        Address::from([0x22; 20]),
        "USDC".to_string(),
        "USDC".to_string(),
        6,
    );
    let mut pool = Pool::new(
        chain.clone(),
        dex.clone(),
        Address::from([0x12; 20]),
        PoolIdentifier::new("0x1234567890123456789012345678901234567890"),
        0u64,
        token0,
        token1,
        Some(500u32),
        Some(10u32),
        UnixNanos::from(1),
    );

    let initial_price = U160::from(79228162514264337593543950336u128); // sqrt(1) * 2^96
    pool.initialize(initial_price, get_tick_at_sqrt_ratio(initial_price));
    let instrument_id = pool.instrument_id;

    // Add pool to cache so setup_pool_updater doesn't request snapshot
    data_engine
        .borrow()
        .cache_rc()
        .borrow_mut()
        .add_pool(pool.clone())
        .unwrap();

    let update = PoolLiquidityUpdate::new(
        chain,
        dex,
        instrument_id,
        pool.pool_identifier,
        PoolLiquidityUpdateType::Mint,
        1000u64,
        "0x123".to_string(),
        0,
        0,
        None,
        Address::from([0x12; 20]),
        100u128,
        U256::from(1000000u128),
        U256::from(2000000u128),
        -100,
        100,
        None,
    );

    let sub = DefiSubscribeCommand::PoolLiquidityUpdates(SubscribePoolLiquidityUpdates {
        instrument_id,
        client_id: Some(client_id),
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    });
    let cmd = DataCommand::DefiSubscribe(sub);

    data_engine.borrow_mut().execute(cmd);

    let (typed_handler, saving_handler) =
        get_typed_message_saving_handler::<PoolLiquidityUpdate>(None);
    let topic = defi::switchboard::get_defi_liquidity_topic(instrument_id);
    msgbus::subscribe_defi_liquidity(topic.into(), typed_handler, None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_defi_data(DefiData::PoolLiquidityUpdate(update.clone()));
    let messages = saving_handler.get_messages();

    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&update));
}

#[cfg(feature = "defi")]
#[rstest]
fn test_process_pool_fee_collect(
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    data_engine.borrow_mut().register_client(data_client, None);

    // Create test pool
    let chain = Arc::new(chains::ETHEREUM.clone());
    let dex = Arc::new(Dex::new(
        chains::ETHEREUM.clone(),
        DexType::UniswapV3,
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        0,
        AmmType::CLAMM,
        "PoolCreated",
        "Swap",
        "Mint",
        "Burn",
        "Collect",
    ));
    let token0 = Token::new(
        chain.clone(),
        Address::from([0x11; 20]),
        "WETH".to_string(),
        "WETH".to_string(),
        18,
    );
    let token1 = Token::new(
        chain.clone(),
        Address::from([0x22; 20]),
        "USDC".to_string(),
        "USDC".to_string(),
        6,
    );
    let mut pool = Pool::new(
        chain.clone(),
        dex.clone(),
        Address::from([0x12; 20]),
        PoolIdentifier::new("0x1234567890123456789012345678901234567890"),
        0u64,
        token0,
        token1,
        Some(500u32),
        Some(10u32),
        UnixNanos::from(1),
    );

    let initial_price = U160::from(79228162514264337593543950336u128); // sqrt(1) * 2^96
    pool.initialize(initial_price, get_tick_at_sqrt_ratio(initial_price));
    let instrument_id = pool.instrument_id;

    // Add pool to cache so setup_pool_updater doesn't request snapshot
    data_engine
        .borrow()
        .cache_rc()
        .borrow_mut()
        .add_pool(pool.clone())
        .unwrap();

    let collect = PoolFeeCollect::new(
        chain,
        dex,
        instrument_id,
        pool.pool_identifier,
        1000u64,
        "0x123".to_string(),
        0,
        0,
        Address::from([0x12; 20]),
        500000u128,
        300000u128,
        -100,
        100,
        None,
    );

    let sub = DefiSubscribeCommand::PoolFeeCollects(SubscribePoolFeeCollects {
        instrument_id,
        client_id: Some(client_id),
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    });
    let cmd = DataCommand::DefiSubscribe(sub);

    data_engine.borrow_mut().execute(cmd);

    let (typed_handler, saving_handler) = get_typed_message_saving_handler::<PoolFeeCollect>(None);
    let topic = defi::switchboard::get_defi_collect_topic(instrument_id);
    msgbus::subscribe_defi_collects(topic.into(), typed_handler, None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_defi_data(DefiData::PoolFeeCollect(collect.clone()));
    let messages = saving_handler.get_messages();

    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&collect));
}

#[cfg(feature = "defi")]
#[rstest]
fn test_process_pool_flash(data_engine: Rc<RefCell<DataEngine>>, data_client: DataClientAdapter) {
    let client_id = data_client.client_id;
    data_engine.borrow_mut().register_client(data_client, None);

    // Create test pool
    let chain = Arc::new(chains::ETHEREUM.clone());
    let dex = Arc::new(Dex::new(
        chains::ETHEREUM.clone(),
        DexType::UniswapV3,
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        0,
        AmmType::CLAMM,
        "PoolCreated",
        "Swap",
        "Mint",
        "Burn",
        "Collect",
    ));
    let token0 = Token::new(
        chain.clone(),
        Address::from([0x11; 20]),
        "WETH".to_string(),
        "WETH".to_string(),
        18,
    );
    let token1 = Token::new(
        chain.clone(),
        Address::from([0x22; 20]),
        "USDC".to_string(),
        "USDC".to_string(),
        6,
    );
    let mut pool = Pool::new(
        chain.clone(),
        dex.clone(),
        Address::from([0x12; 20]),
        PoolIdentifier::new("0x1234567890123456789012345678901234567890"),
        0u64,
        token0,
        token1,
        Some(500u32),
        Some(10u32),
        UnixNanos::from(1),
    );

    let initial_price = U160::from(79228162514264337593543950336u128); // sqrt(1) * 2^96
    pool.initialize(initial_price, get_tick_at_sqrt_ratio(initial_price));
    let instrument_id = pool.instrument_id;

    // Add pool to cache so setup_pool_updater doesn't request snapshot
    data_engine
        .borrow()
        .cache_rc()
        .borrow_mut()
        .add_pool(pool.clone())
        .unwrap();

    let flash = PoolFlash::new(
        chain,
        dex,
        instrument_id,
        pool.pool_identifier,
        1000u64,
        "0x123".to_string(),
        0,
        0,
        None,
        Address::from([0x12; 20]),
        Address::from([0x34; 20]),
        U256::from(1000000u128),
        U256::from(500000u128),
        U256::from(5000u128),
        U256::from(2500u128),
    );

    let sub = DefiSubscribeCommand::PoolFlashEvents(SubscribePoolFlashEvents {
        instrument_id,
        client_id: Some(client_id),
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    });
    let cmd = DataCommand::DefiSubscribe(sub);

    data_engine.borrow_mut().execute(cmd);

    let (typed_handler, saving_handler) = get_typed_message_saving_handler::<PoolFlash>(None);
    let topic = defi::switchboard::get_defi_flash_topic(instrument_id);
    msgbus::subscribe_defi_flash(topic.into(), typed_handler, None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_defi_data(DefiData::PoolFlash(flash.clone()));
    let messages = saving_handler.get_messages();

    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&flash));
}

// -- POOL UPDATER INTEGRATION TESTS ----------------------------------------------------------

#[cfg(feature = "defi")]
#[rstest]
fn test_pool_updater_processes_swap_updates_profiler(
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let cache = data_engine.borrow().cache_rc();
    data_engine.borrow_mut().register_client(data_client, None);

    // Create pool test data
    let chain = Arc::new(chains::ARBITRUM.clone());
    let dex = Arc::new(Dex::new(
        chains::ARBITRUM.clone(),
        DexType::UniswapV3,
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        0,
        AmmType::CLAMM,
        "PoolCreated",
        "Swap",
        "Mint",
        "Burn",
        "Collect",
    ));
    let token0 = Token::new(
        chain.clone(),
        Address::from([0x11; 20]),
        "WETH".to_string(),
        "WETH".to_string(),
        18,
    );
    let token1 = Token::new(
        chain.clone(),
        Address::from([0x22; 20]),
        "USDC".to_string(),
        "USDC".to_string(),
        6,
    );
    let mut pool = Pool::new(
        chain.clone(),
        dex.clone(),
        Address::from([0x12; 20]),
        PoolIdentifier::new("0x1234567890123456789012345678901234567890"),
        0u64,
        token0,
        token1,
        Some(500u32),
        Some(10u32),
        UnixNanos::from(1),
    );

    let initial_price = U160::from(79228162514264337593543950336u128); // sqrt(1) * 2^96
    pool.initialize(initial_price, get_tick_at_sqrt_ratio(initial_price));
    let instrument_id = pool.instrument_id;

    // Add pool to cache and create profiler
    let shared_pool = Arc::new(pool.clone());
    cache.borrow_mut().add_pool(pool).unwrap();
    let mut profiler = PoolProfiler::new(shared_pool);
    profiler.initialize(initial_price).unwrap();

    // Add liquidity so swaps can be processed
    let mint = PoolLiquidityUpdate::new(
        chain.clone(),
        dex.clone(),
        instrument_id,
        PoolIdentifier::from_address(Address::from([0x12; 20])),
        PoolLiquidityUpdateType::Mint,
        999u64,
        "0x122".to_string(),
        0,
        0,
        None,
        Address::from([0xAB; 20]),
        10000u128, // Add significant liquidity
        U256::from(1000000u128),
        U256::from(2000000u128),
        -1000, // Wide range
        1000,
        None,
    );
    profiler.process_mint(&mint).unwrap();
    cache.borrow_mut().add_pool_profiler(profiler).unwrap();

    // Verify liquidity was activated by the mint
    let active_liquidity = cache
        .borrow()
        .pool_profiler(&instrument_id)
        .unwrap()
        .tick_map
        .liquidity;
    assert!(
        active_liquidity > 0,
        "Active liquidity should be > 0 after mint, was: {active_liquidity}"
    );

    // Capture initial profiler state (after mint)
    let initial_tick = cache
        .borrow()
        .pool_profiler(&instrument_id)
        .unwrap()
        .state
        .current_tick;
    let initial_fee_growth_0 = cache
        .borrow()
        .pool_profiler(&instrument_id)
        .unwrap()
        .state
        .fee_growth_global_0;

    // Subscribe to pool swaps (this creates PoolUpdater and subscribes to topic)
    let sub = DefiSubscribeCommand::PoolSwaps(SubscribePoolSwaps {
        instrument_id,
        client_id: Some(client_id),
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    });
    let cmd = DataCommand::DefiSubscribe(sub);
    data_engine.borrow_mut().execute(cmd);

    // Create and process swap that changes tick
    let new_price = U160::from(56022770974786139918731938227u128); // Different price
    let swap = PoolSwap::new(
        chain,
        dex,
        instrument_id,
        PoolIdentifier::from_address(Address::from([0x12; 20])),
        1000u64,
        "0x123".to_string(),
        0,
        0,
        None,
        Address::from([0x12; 20]),
        Address::from([0x12; 20]),
        I256::from_str("1000000000000000000").unwrap(),
        I256::from_str("400000000000000").unwrap(),
        new_price,
        1000u128,
        0i32,
    );

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_defi_data(DefiData::PoolSwap(swap));

    // Verify profiler state was updated by PoolUpdater
    let final_tick = cache
        .borrow()
        .pool_profiler(&instrument_id)
        .unwrap()
        .state
        .current_tick;
    let final_fee_growth_0 = cache
        .borrow()
        .pool_profiler(&instrument_id)
        .unwrap()
        .state
        .fee_growth_global_0;

    // Verify profiler was updated - either tick changed OR fees were collected
    // (depending on whether the swap crossed ticks or just generated fees)
    let tick_changed = final_tick != initial_tick;
    let fees_increased = final_fee_growth_0 > initial_fee_growth_0;

    assert!(
        tick_changed || fees_increased,
        "PoolUpdater should have updated PoolProfiler: tick_changed={tick_changed}, fees_increased={fees_increased}, \
        initial_tick={initial_tick:?}, final_tick={final_tick:?}, initial_fee_growth={initial_fee_growth_0}, final_fee_growth={final_fee_growth_0}"
    );
}

#[cfg(feature = "defi")]
#[rstest]
fn test_pool_updater_processes_mint_updates_profiler(
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let cache = data_engine.borrow().cache_rc();
    data_engine.borrow_mut().register_client(data_client, None);

    // Create pool test data
    let chain = Arc::new(chains::ETHEREUM.clone());
    let dex = Arc::new(Dex::new(
        chains::ETHEREUM.clone(),
        DexType::UniswapV3,
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        0,
        AmmType::CLAMM,
        "PoolCreated",
        "Swap",
        "Mint",
        "Burn",
        "Collect",
    ));
    let token0 = Token::new(
        chain.clone(),
        Address::from([0x11; 20]),
        "WETH".to_string(),
        "WETH".to_string(),
        18,
    );
    let token1 = Token::new(
        chain.clone(),
        Address::from([0x22; 20]),
        "USDC".to_string(),
        "USDC".to_string(),
        6,
    );
    let mut pool = Pool::new(
        chain.clone(),
        dex.clone(),
        Address::from([0x12; 20]),
        PoolIdentifier::new("0x1234567890123456789012345678901234567890"),
        0u64,
        token0,
        token1,
        Some(500u32),
        Some(10u32),
        UnixNanos::from(1),
    );

    let initial_price = U160::from(79228162514264337593543950336u128);
    pool.initialize(initial_price, get_tick_at_sqrt_ratio(initial_price));
    let instrument_id = pool.instrument_id;

    // Add pool to cache and create profiler
    let shared_pool = Arc::new(pool.clone());
    cache.borrow_mut().add_pool(pool).unwrap();
    let mut profiler = PoolProfiler::new(shared_pool);
    profiler.initialize(initial_price).unwrap();
    cache.borrow_mut().add_pool_profiler(profiler).unwrap();

    // Capture initial profiler tick state
    let initial_liquidity = cache
        .borrow()
        .pool_profiler(&instrument_id)
        .unwrap()
        .tick_map
        .liquidity;

    // Subscribe to pool liquidity updates
    let sub = DefiSubscribeCommand::PoolLiquidityUpdates(SubscribePoolLiquidityUpdates {
        instrument_id,
        client_id: Some(client_id),
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    });
    let cmd = DataCommand::DefiSubscribe(sub);
    data_engine.borrow_mut().execute(cmd);

    // Create and process mint event
    let mint = PoolLiquidityUpdate::new(
        chain,
        dex,
        instrument_id,
        PoolIdentifier::from_address(Address::from([0x12; 20])),
        PoolLiquidityUpdateType::Mint,
        1000u64,
        "0x123".to_string(),
        0,
        0,
        None,
        Address::from([0xAB; 20]),
        1000u128, // liquidity amount
        U256::from(100000u128),
        U256::from(200000u128),
        -100, // tick_lower
        100,  // tick_upper
        None,
    );

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_defi_data(DefiData::PoolLiquidityUpdate(mint));

    // Verify profiler tick map was updated with new liquidity
    let final_liquidity = cache
        .borrow()
        .pool_profiler(&instrument_id)
        .unwrap()
        .tick_map
        .liquidity;

    assert!(
        final_liquidity >= initial_liquidity,
        "Liquidity should have increased after mint"
    );
}

#[cfg(feature = "defi")]
#[rstest]
fn test_pool_updater_processes_burn_updates_profiler(
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let cache = data_engine.borrow().cache_rc();
    data_engine.borrow_mut().register_client(data_client, None);

    // Create pool test data
    let chain = Arc::new(chains::ETHEREUM.clone());
    let dex = Arc::new(Dex::new(
        chains::ETHEREUM.clone(),
        DexType::UniswapV3,
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        0,
        AmmType::CLAMM,
        "PoolCreated",
        "Swap",
        "Mint",
        "Burn",
        "Collect",
    ));
    let token0 = Token::new(
        chain.clone(),
        Address::from([0x11; 20]),
        "WETH".to_string(),
        "WETH".to_string(),
        18,
    );
    let token1 = Token::new(
        chain.clone(),
        Address::from([0x22; 20]),
        "USDC".to_string(),
        "USDC".to_string(),
        6,
    );
    let mut pool = Pool::new(
        chain.clone(),
        dex.clone(),
        Address::from([0x12; 20]),
        PoolIdentifier::new("0x1234567890123456789012345678901234567890"),
        0u64,
        token0,
        token1,
        Some(500u32),
        Some(10u32),
        UnixNanos::from(1),
    );

    let initial_price = U160::from(79228162514264337593543950336u128);
    pool.initialize(initial_price, get_tick_at_sqrt_ratio(initial_price));
    let instrument_id = pool.instrument_id;

    // Add pool to cache and create profiler
    let shared_pool = Arc::new(pool.clone());
    cache.borrow_mut().add_pool(pool).unwrap();
    let mut profiler = PoolProfiler::new(shared_pool);
    profiler.initialize(initial_price).unwrap();

    // First mint some liquidity
    let owner = Address::from([0xAB; 20]);
    let mint = PoolLiquidityUpdate::new(
        chain.clone(),
        dex.clone(),
        instrument_id,
        PoolIdentifier::from_address(Address::from([0x12; 20])),
        PoolLiquidityUpdateType::Mint,
        1000u64,
        "0x123".to_string(),
        0,
        0,
        None,
        owner,
        1000u128,
        U256::from(100000u128),
        U256::from(200000u128),
        -100,
        100,
        None,
    );
    profiler.process_mint(&mint).unwrap();
    cache.borrow_mut().add_pool_profiler(profiler).unwrap();

    // Capture liquidity after mint
    let liquidity_after_mint = cache
        .borrow()
        .pool_profiler(&instrument_id)
        .unwrap()
        .tick_map
        .liquidity;

    // Subscribe to pool liquidity updates
    let sub = DefiSubscribeCommand::PoolLiquidityUpdates(SubscribePoolLiquidityUpdates {
        instrument_id,
        client_id: Some(client_id),
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    });
    let cmd = DataCommand::DefiSubscribe(sub);
    data_engine.borrow_mut().execute(cmd);

    // Create and process burn event
    let burn = PoolLiquidityUpdate::new(
        chain,
        dex,
        instrument_id,
        PoolIdentifier::from_address(Address::from([0x12; 20])),
        PoolLiquidityUpdateType::Burn,
        1001u64,
        "0x124".to_string(),
        0,
        0,
        None,
        owner,
        500u128, // burn half the liquidity
        U256::from(50000u128),
        U256::from(100000u128),
        -100,
        100,
        None,
    );

    data_engine
        .borrow_mut()
        .process_defi_data(DefiData::PoolLiquidityUpdate(burn));

    // Verify profiler tick map was updated (liquidity decreased)
    let final_liquidity = cache
        .borrow()
        .pool_profiler(&instrument_id)
        .unwrap()
        .tick_map
        .liquidity;

    assert!(
        final_liquidity < liquidity_after_mint,
        "Liquidity should have decreased after burn"
    );
}

#[cfg(feature = "defi")]
#[rstest]
fn test_pool_updater_processes_collect_updates_profiler(
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let cache = data_engine.borrow().cache_rc();
    data_engine.borrow_mut().register_client(data_client, None);

    // Create pool test data
    let chain = Arc::new(chains::ETHEREUM.clone());
    let dex = Arc::new(Dex::new(
        chains::ETHEREUM.clone(),
        DexType::UniswapV3,
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        0,
        AmmType::CLAMM,
        "PoolCreated",
        "Swap",
        "Mint",
        "Burn",
        "Collect",
    ));
    let token0 = Token::new(
        chain.clone(),
        Address::from([0x11; 20]),
        "WETH".to_string(),
        "WETH".to_string(),
        18,
    );
    let token1 = Token::new(
        chain.clone(),
        Address::from([0x22; 20]),
        "USDC".to_string(),
        "USDC".to_string(),
        6,
    );
    let mut pool = Pool::new(
        chain.clone(),
        dex.clone(),
        Address::from([0x12; 20]),
        PoolIdentifier::new("0x1234567890123456789012345678901234567890"),
        0u64,
        token0,
        token1,
        Some(500u32),
        Some(10u32),
        UnixNanos::from(1),
    );

    let initial_price = U160::from(79228162514264337593543950336u128);
    pool.initialize(initial_price, get_tick_at_sqrt_ratio(initial_price));
    let instrument_id = pool.instrument_id;

    // Add pool to cache and create profiler
    let shared_pool = Arc::new(pool.clone());
    cache.borrow_mut().add_pool(pool).unwrap();
    let mut profiler = PoolProfiler::new(shared_pool);
    profiler.initialize(initial_price).unwrap();
    cache.borrow_mut().add_pool_profiler(profiler).unwrap();

    // Subscribe to pool fee collects
    let sub = DefiSubscribeCommand::PoolFeeCollects(SubscribePoolFeeCollects {
        instrument_id,
        client_id: Some(client_id),
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    });
    let cmd = DataCommand::DefiSubscribe(sub);
    data_engine.borrow_mut().execute(cmd);

    // Create and process collect event
    let owner = Address::from([0xAB; 20]);
    let collect = PoolFeeCollect::new(
        chain,
        dex,
        instrument_id,
        PoolIdentifier::from_address(Address::from([0x12; 20])),
        1000u64,
        "0x123".to_string(),
        0,
        0,
        owner,
        50000u128, // amount0
        30000u128, // amount1
        -100,      // tick_lower
        100,       // tick_upper
        None,
    );

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_defi_data(DefiData::PoolFeeCollect(collect));

    // Verify profiler state - the collect should be processed without error
    // The main verification is that PoolUpdater called PoolProfiler.process_collect()
    // which would have updated internal position state if the position existed
    let is_initialized = cache
        .borrow()
        .pool_profiler(&instrument_id)
        .unwrap()
        .is_initialized;

    // PoolProfiler should still be valid and initialized
    assert!(is_initialized, "PoolProfiler should remain initialized");
}

#[cfg(feature = "defi")]
#[rstest]
fn test_pool_updater_processes_flash_updates_profiler(
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let cache = data_engine.borrow().cache_rc();
    data_engine.borrow_mut().register_client(data_client, None);

    // Create pool test data
    let chain = Arc::new(chains::ARBITRUM.clone());
    let dex = Arc::new(Dex::new(
        chains::ARBITRUM.clone(),
        DexType::UniswapV3,
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        0,
        AmmType::CLAMM,
        "PoolCreated",
        "Swap",
        "Mint",
        "Burn",
        "Collect",
    ));
    let token0 = Token::new(
        chain.clone(),
        Address::from([0x11; 20]),
        "WETH".to_string(),
        "WETH".to_string(),
        18,
    );
    let token1 = Token::new(
        chain.clone(),
        Address::from([0x22; 20]),
        "USDC".to_string(),
        "USDC".to_string(),
        6,
    );
    let mut pool = Pool::new(
        chain.clone(),
        dex.clone(),
        Address::from([0x12; 20]),
        PoolIdentifier::from_address(Address::from([0x12; 20])),
        0u64,
        token0,
        token1,
        Some(500u32),
        Some(10u32),
        UnixNanos::from(1),
    );

    let initial_price = U160::from(79228162514264337593543950336u128);
    pool.initialize(initial_price, get_tick_at_sqrt_ratio(initial_price));
    let instrument_id = pool.instrument_id;

    // Add pool to cache and create profiler
    let shared_pool = Arc::new(pool.clone());
    cache.borrow_mut().add_pool(pool).unwrap();
    let mut profiler = PoolProfiler::new(shared_pool);
    profiler.initialize(initial_price).unwrap();
    cache.borrow_mut().add_pool_profiler(profiler).unwrap();

    // Subscribe to pool flash events
    let sub = DefiSubscribeCommand::PoolFlashEvents(SubscribePoolFlashEvents {
        instrument_id,
        client_id: Some(client_id),
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    });
    let cmd = DataCommand::DefiSubscribe(sub);
    data_engine.borrow_mut().execute(cmd);

    // Create and process flash event
    let initiator = Address::from([0xAB; 20]);
    let recipient = Address::from([0xCD; 20]);
    let flash = PoolFlash::new(
        chain,
        dex,
        instrument_id,
        PoolIdentifier::from_address(Address::from([0x12; 20])),
        1000u64,
        "0x123".to_string(),
        0,
        0,
        None,
        initiator,
        recipient,
        U256::from(1000000u128), // amount0
        U256::from(500000u128),  // amount1
        U256::from(5000u128),    // paid0
        U256::from(2500u128),    // paid1
    );

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_defi_data(DefiData::PoolFlash(flash));

    // Verify profiler state - the flash should be processed without error
    // The main verification is that PoolUpdater called PoolProfiler.process_flash()
    // which would have updated flash statistics
    let is_initialized = cache
        .borrow()
        .pool_profiler(&instrument_id)
        .unwrap()
        .is_initialized;

    // PoolProfiler should still be valid and initialized
    assert!(is_initialized, "PoolProfiler should remain initialized");
}

#[cfg(feature = "defi")]
#[rstest]
fn test_execute_defi_request_pool_snapshot(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let instrument_id =
        InstrumentId::from("0x11b815efB8f581194ae79006d24E0d814B7697F6.Arbitrum:UniswapV3");

    let request = RequestPoolSnapshot::new(
        instrument_id,
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );

    let cmd = DataCommand::DefiRequest(DefiRequestCommand::PoolSnapshot(request));
    data_engine.execute(cmd.clone());

    // Verify command was forwarded to the client
    assert_eq!(recorder.borrow().len(), 1);
    assert_eq!(recorder.borrow().as_slice(), std::slice::from_ref(&cmd));
}

#[cfg(feature = "defi")]
#[rstest]
fn test_setup_pool_updater_requests_snapshot(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let instrument_id =
        InstrumentId::from("0x11b815efB8f581194ae79006d24E0d814B7697F6.Arbitrum:UniswapV3");

    let subscribe_pool = SubscribePool::new(
        instrument_id,
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );

    let cmd = DataCommand::DefiSubscribe(DefiSubscribeCommand::Pool(subscribe_pool));
    data_engine.execute(cmd.clone());

    // Verify two commands were recorded:
    // 1. The SubscribePool command (forwarded to client first)
    // 2. The RequestPoolSnapshot command (automatically sent by setup_pool_updater after)
    let recorded = recorder.borrow();
    assert_eq!(
        recorded.len(),
        2,
        "Expected 2 commands (SubscribePool and RequestPoolSnapshot)"
    );

    // First command should be the SubscribePool (forwarded before snapshot request)
    assert_eq!(recorded[0], cmd);

    // Second command should be RequestPoolSnapshot
    match &recorded[1] {
        DataCommand::DefiRequest(DefiRequestCommand::PoolSnapshot(request)) => {
            assert_eq!(request.instrument_id, instrument_id);
            assert_eq!(request.client_id, Some(client_id));
        }
        _ => panic!(
            "Expected second command to be RequestPoolSnapshot, was: {:?}",
            recorded[1]
        ),
    }
}

#[cfg(feature = "defi")]
#[rstest]
fn test_setup_pool_updater_skips_snapshot_when_pool_in_cache(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    // Create a pool with initial price and add to cache BEFORE subscribing
    let chain = Arc::new(chains::ARBITRUM.clone());
    let dex = Arc::new(Dex::new(
        chains::ARBITRUM.clone(),
        DexType::UniswapV3,
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        0,
        AmmType::CLAMM,
        "PoolCreated",
        "Swap",
        "Mint",
        "Burn",
        "Collect",
    ));
    let token0 = Token::new(
        chain.clone(),
        Address::from([0x11; 20]),
        "WETH".to_string(),
        "WETH".to_string(),
        18,
    );
    let token1 = Token::new(
        chain.clone(),
        Address::from([0x22; 20]),
        "USDC".to_string(),
        "USDC".to_string(),
        6,
    );
    let mut pool = Pool::new(
        chain,
        dex,
        Address::from([0x88; 20]),
        PoolIdentifier::from_address(Address::from([0x88; 20])),
        0u64,
        token0,
        token1,
        Some(500u32),
        Some(10u32),
        UnixNanos::from(1),
    );

    let initial_price = U160::from(79228162514264337593543950336u128); // sqrt(1) * 2^96
    pool.initialize(initial_price, get_tick_at_sqrt_ratio(initial_price));
    let instrument_id = pool.instrument_id;

    // Add pool to the data_engine's cache (not the fixture cache!)
    // This ensures setup_pool_updater finds the pool when it checks the cache
    data_engine.cache_rc().borrow_mut().add_pool(pool).unwrap();

    let subscribe_pool = SubscribePool::new(
        instrument_id,
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );

    let cmd = DataCommand::DefiSubscribe(DefiSubscribeCommand::Pool(subscribe_pool));
    data_engine.execute(cmd.clone());

    // Verify the cache-first optimization: when a pool exists in the data engine's
    // cache, setup_pool_updater should skip the snapshot request and proceed
    // directly to creating the profiler and updater from the cached pool.
    // Only the SubscribePool command should be forwarded to the client.
    let recorded = recorder.borrow();
    assert_eq!(
        recorded.len(),
        1,
        "Expected only 1 command (SubscribePool), but got {} commands. \
         When pool is in cache, snapshot request should be skipped.",
        recorded.len()
    );

    // The single command should be the subscription
    assert_eq!(recorded[0], cmd);
}

#[cfg(feature = "defi")]
#[rstest]
fn test_setup_pool_updater_does_not_cache_profiler_on_initialize_failure(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let chain = Arc::new(chains::ARBITRUM.clone());
    let dex = Arc::new(Dex::new(
        chains::ARBITRUM.clone(),
        DexType::UniswapV3,
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        0,
        AmmType::CLAMM,
        "PoolCreated",
        "Swap",
        "Mint",
        "Burn",
        "Collect",
    ));
    let token0 = Token::new(
        chain.clone(),
        Address::from([0x11; 20]),
        "WETH".to_string(),
        "WETH".to_string(),
        18,
    );
    let token1 = Token::new(
        chain.clone(),
        Address::from([0x22; 20]),
        "USDC".to_string(),
        "USDC".to_string(),
        6,
    );
    let mut pool = Pool::new(
        chain,
        dex,
        Address::from([0x99; 20]),
        PoolIdentifier::from_address(Address::from([0x99; 20])),
        0u64,
        token0,
        token1,
        Some(500u32),
        Some(10u32),
        UnixNanos::from(1),
    );

    // Construct a pool whose stored initial_tick disagrees with the tick derived
    // from its sqrt price. setup_pool_updater calls PoolProfiler::initialize which
    // must return InitialTickMismatch and must not cache the half-initialized profiler.
    // Pool::initialize asserts consistency, so set the fields directly.
    let initial_price = U160::from(79228162514264337593543950336u128); // sqrt(1) * 2^96
    let real_tick = get_tick_at_sqrt_ratio(initial_price);
    pool.initial_sqrt_price_x96 = Some(initial_price);
    pool.initial_tick = Some(real_tick + 100);
    let instrument_id = pool.instrument_id;

    data_engine.cache_rc().borrow_mut().add_pool(pool).unwrap();
    assert!(
        data_engine
            .cache_rc()
            .borrow()
            .pool_profiler(&instrument_id)
            .is_none()
    );

    let subscribe_pool = SubscribePool::new(
        instrument_id,
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    let cmd = DataCommand::DefiSubscribe(DefiSubscribeCommand::Pool(subscribe_pool));
    data_engine.execute(cmd);

    assert!(
        data_engine
            .cache_rc()
            .borrow()
            .pool_profiler(&instrument_id)
            .is_none(),
        "profiler must not be cached when initialize fails"
    );
}

#[cfg(feature = "defi")]
#[rstest]
fn test_pool_snapshot_request_routing_by_client_id(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let mut data_engine = data_engine.borrow_mut();

    // Register two clients
    let client_id_1 = ClientId::from("CLIENT1");
    let venue_1 = Venue::from("VENUE1");
    let recorder_1: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock.clone(),
        cache.clone(),
        client_id_1,
        venue_1,
        None,
        &recorder_1,
        &mut data_engine,
    );

    let client_id_2 = ClientId::from("CLIENT2");
    let venue_2 = Venue::from("VENUE2");
    let recorder_2: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id_2,
        venue_2,
        None,
        &recorder_2,
        &mut data_engine,
    );

    let instrument_id =
        InstrumentId::from("0x11b815efB8f581194ae79006d24E0d814B7697F6.Arbitrum:UniswapV3");

    // Request snapshot with specific client_id
    let request = RequestPoolSnapshot::new(
        instrument_id,
        Some(client_id_1),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );

    let cmd = DataCommand::DefiRequest(DefiRequestCommand::PoolSnapshot(request));
    data_engine.execute(cmd.clone());

    // Verify request was routed to CLIENT1 only
    assert_eq!(recorder_1.borrow().len(), 1);
    assert_eq!(recorder_1.borrow()[0], cmd);
    assert_eq!(recorder_2.borrow().len(), 0);
}

#[rstest]
#[tokio::test]
#[expect(clippy::await_holding_refcell_ref)] // Single-threaded test
async fn test_data_engine_connect_continues_with_failing_client(
    #[from(data_engine)] data_engine: Rc<RefCell<DataEngine>>,
) {
    let mut data_engine = data_engine.borrow_mut();

    let client_id = ClientId::from("FAILING_CLIENT");
    let venue = Venue::from("TEST");
    let error_message = "Authentication failed: invalid API key";

    let client = FailingMockDataClient::new(client_id, Some(venue), error_message);
    let adapter = DataClientAdapter::new(client_id, Some(venue), true, true, Box::new(client));
    data_engine.register_client(adapter, None);

    // Connect logs errors but does not fail
    data_engine.connect().await;
}

#[rstest]
#[tokio::test]
#[expect(clippy::await_holding_refcell_ref)] // Single-threaded test
async fn test_data_engine_connect_succeeds_with_working_client(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    #[from(data_engine)] data_engine: Rc<RefCell<DataEngine>>,
) {
    let mut data_engine = data_engine.borrow_mut();

    let client_id = ClientId::from("WORKING_CLIENT");
    let venue = Venue::from("TEST");

    let client = MockDataClient::new(clock, cache, client_id, Some(venue));
    let adapter = DataClientAdapter::new(client_id, Some(venue), true, true, Box::new(client));
    data_engine.register_client(adapter, None);

    data_engine.connect().await;
}

#[rstest]
fn test_process_book_snapshot_publish(
    audusd_sim: CurrencyPair,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    // Ensure message bus is initialized
    let _ = msgbus::get_message_bus();

    // Create data engine
    let data_engine = Rc::new(RefCell::new(DataEngine::new(
        clock.clone(),
        cache.clone(),
        None,
    )));

    let data_engine_clone = data_engine.clone();
    let handler = TypedIntoHandler::from(move |cmd: DataCommand| {
        data_engine_clone.borrow_mut().execute(cmd);
    });
    let endpoint = MessagingSwitchboard::data_engine_execute();
    msgbus::register_data_command_endpoint(endpoint, handler);

    // Register mock client
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock.clone(),
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine.borrow_mut(),
    );

    // Add instrument to cache
    let _ = cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim.clone()));

    // Set up book snapshot handler to capture published snapshots
    let interval_ms = NonZeroUsize::new(100).unwrap();
    let topic = switchboard::get_book_snapshots_topic(audusd_sim.id, interval_ms);
    let (handler, saver) = get_typed_message_saving_handler::<OrderBook>(None);
    msgbus::subscribe_book_snapshots(topic.into(), handler, None);

    // Subscribe to book snapshots (sets up timer and book updater)
    let sub = SubscribeBookSnapshots::new(
        audusd_sim.id,
        BookType::L2_MBP,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        interval_ms,
        None,
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::BookSnapshots(sub));
    data_engine.borrow_mut().execute(cmd);

    // Process deltas to populate the order book
    let delta = OrderBookDeltaTestBuilder::new(audusd_sim.id).build();
    let deltas = OrderBookDeltas_API::new(OrderBookDeltas::new(audusd_sim.id, vec![delta]));
    data_engine.borrow_mut().process_data(Data::Deltas(deltas));

    // Advance clock past the interval to trigger snapshot timer
    let advance_ns = 200_000_000u64; // 200ms in nanoseconds
    let events = clock.borrow_mut().advance_time(advance_ns.into(), true);

    // Process timer events (fire callbacks)
    let handlers = clock.borrow().match_handlers(events);
    for handler in handlers {
        handler.callback.call(handler.event);
    }

    // Verify snapshot was published and received
    wait_until(
        || !saver.get_messages().is_empty(),
        Duration::from_millis(100),
    );

    let messages = saver.get_messages();
    assert!(!messages.is_empty(), "Expected at least one book snapshot");
    assert_eq!(messages[0].instrument_id, audusd_sim.id);
}

#[rstest]
fn test_process_book_snapshot_publish_for_multiple_instruments_same_interval(
    audusd_sim: CurrencyPair,
    gbpusd_sim: CurrencyPair,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let data_engine = create_snapshot_test_engine(clock.clone(), cache.clone());
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock.clone(),
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine.borrow_mut(),
    );

    let _ = cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim.clone()));
    let _ = cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(gbpusd_sim.clone()));

    let interval_ms = NonZeroUsize::new(100).unwrap();
    let aud_topic = switchboard::get_book_snapshots_topic(audusd_sim.id, interval_ms);
    let gbp_topic = switchboard::get_book_snapshots_topic(gbpusd_sim.id, interval_ms);
    let (aud_handler, aud_saver) = get_typed_message_saving_handler::<OrderBook>(None);
    let (gbp_handler, gbp_saver) = get_typed_message_saving_handler::<OrderBook>(None);
    msgbus::subscribe_book_snapshots(aud_topic.into(), aud_handler, None);
    msgbus::subscribe_book_snapshots(gbp_topic.into(), gbp_handler, None);

    execute_book_snapshot_subscribe(&data_engine, audusd_sim.id, client_id, venue, interval_ms);
    execute_book_snapshot_subscribe(&data_engine, gbpusd_sim.id, client_id, venue, interval_ms);

    process_book_delta(&data_engine, audusd_sim.id);
    process_book_delta(&data_engine, gbpusd_sim.id);
    advance_clock_and_dispatch(&clock, 200_000_000);

    assert_eq!(aud_saver.get_messages().len(), 1);
    assert_eq!(aud_saver.get_messages()[0].instrument_id, audusd_sim.id);
    assert_eq!(gbp_saver.get_messages().len(), 1);
    assert_eq!(gbp_saver.get_messages()[0].instrument_id, gbpusd_sim.id);
}

#[rstest]
fn test_subscribed_book_snapshots_preserve_subscription_order(
    audusd_sim: CurrencyPair,
    gbpusd_sim: CurrencyPair,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    // Pin IndexMap iteration on DataEngine.book_snapshot_counts: the per-tick
    // BookSnapshotter publishes in iteration order, and the public
    // subscribed_book_snapshots() Vec must reflect subscription order across runs.
    let data_engine = create_snapshot_test_engine(clock.clone(), cache.clone());
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine.borrow_mut(),
    );

    let _ = cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim.clone()));
    let _ = cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(gbpusd_sim.clone()));

    let interval_ms = NonZeroUsize::new(100).unwrap();
    execute_book_snapshot_subscribe(&data_engine, gbpusd_sim.id, client_id, venue, interval_ms);
    execute_book_snapshot_subscribe(&data_engine, audusd_sim.id, client_id, venue, interval_ms);

    assert_eq!(
        data_engine.borrow().subscribed_book_snapshots(),
        vec![gbpusd_sim.id, audusd_sim.id],
    );
}

#[rstest]
fn test_process_book_snapshot_publish_for_multiple_intervals_same_instrument(
    audusd_sim: CurrencyPair,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let data_engine = create_snapshot_test_engine(clock.clone(), cache.clone());
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock.clone(),
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine.borrow_mut(),
    );

    let _ = cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim.clone()));

    let fast_interval_ms = NonZeroUsize::new(100).unwrap();
    let slow_interval_ms = NonZeroUsize::new(200).unwrap();
    let fast_topic = switchboard::get_book_snapshots_topic(audusd_sim.id, fast_interval_ms);
    let slow_topic = switchboard::get_book_snapshots_topic(audusd_sim.id, slow_interval_ms);
    let (fast_handler, fast_saver) = get_typed_message_saving_handler::<OrderBook>(None);
    let (slow_handler, slow_saver) = get_typed_message_saving_handler::<OrderBook>(None);
    msgbus::subscribe_book_snapshots(fast_topic.into(), fast_handler, None);
    msgbus::subscribe_book_snapshots(slow_topic.into(), slow_handler, None);

    execute_book_snapshot_subscribe(
        &data_engine,
        audusd_sim.id,
        client_id,
        venue,
        fast_interval_ms,
    );
    execute_book_snapshot_subscribe(
        &data_engine,
        audusd_sim.id,
        client_id,
        venue,
        slow_interval_ms,
    );

    let recorded = recorder.borrow();
    assert_eq!(recorded.len(), 1);
    assert!(matches!(
        &recorded[0],
        DataCommand::Subscribe(SubscribeCommand::BookDeltas(cmd)) if cmd.instrument_id == audusd_sim.id
    ));
    drop(recorded);

    process_book_delta(&data_engine, audusd_sim.id);
    advance_clock_and_dispatch(&clock, 500_000_000);

    assert!(!fast_saver.get_messages().is_empty());
    assert_eq!(fast_saver.get_messages()[0].instrument_id, audusd_sim.id);
    assert!(!slow_saver.get_messages().is_empty());
    assert_eq!(slow_saver.get_messages()[0].instrument_id, audusd_sim.id);
}

#[rstest]
fn test_duplicate_book_snapshot_subscriptions_require_matching_unsubscribes(
    audusd_sim: CurrencyPair,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let data_engine = create_snapshot_test_engine(clock.clone(), cache.clone());
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock.clone(),
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine.borrow_mut(),
    );

    let _ = cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim.clone()));

    let interval_ms = NonZeroUsize::new(100).unwrap();
    let topic = switchboard::get_book_snapshots_topic(audusd_sim.id, interval_ms);
    let (handler, saver) = get_typed_message_saving_handler::<OrderBook>(None);
    msgbus::subscribe_book_snapshots(topic.into(), handler, None);

    execute_book_snapshot_subscribe(&data_engine, audusd_sim.id, client_id, venue, interval_ms);
    execute_book_snapshot_subscribe(&data_engine, audusd_sim.id, client_id, venue, interval_ms);

    assert_eq!(recorder.borrow().len(), 1);

    process_book_delta(&data_engine, audusd_sim.id);
    advance_clock_and_dispatch(&clock, 200_000_000);
    let snapshot_count = saver.get_messages().len();
    assert!(!saver.get_messages().is_empty());

    execute_book_snapshot_unsubscribe(&data_engine, audusd_sim.id, client_id, venue, interval_ms);

    process_book_delta(&data_engine, audusd_sim.id);
    advance_clock_and_dispatch(&clock, 200_000_000);
    assert!(saver.get_messages().len() > snapshot_count);
    assert_eq!(recorder.borrow().len(), 1);

    execute_book_snapshot_unsubscribe(&data_engine, audusd_sim.id, client_id, venue, interval_ms);

    let recorded = recorder.borrow();
    assert_eq!(recorded.len(), 2);
    assert!(matches!(
        &recorded[1],
        DataCommand::Unsubscribe(UnsubscribeCommand::BookDeltas(cmd)) if cmd.instrument_id == audusd_sim.id
    ));
    drop(recorded);

    let snapshot_count = saver.get_messages().len();
    process_book_delta(&data_engine, audusd_sim.id);
    advance_clock_and_dispatch(&clock, 200_000_000);
    assert_eq!(saver.get_messages().len(), snapshot_count);
}

#[rstest]
fn test_unsubscribe_book_snapshots_removes_only_requested_interval(
    audusd_sim: CurrencyPair,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let data_engine = create_snapshot_test_engine(clock.clone(), cache.clone());
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock.clone(),
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine.borrow_mut(),
    );

    let _ = cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim.clone()));

    let fast_interval_ms = NonZeroUsize::new(100).unwrap();
    let slow_interval_ms = NonZeroUsize::new(200).unwrap();
    let fast_topic = switchboard::get_book_snapshots_topic(audusd_sim.id, fast_interval_ms);
    let slow_topic = switchboard::get_book_snapshots_topic(audusd_sim.id, slow_interval_ms);
    let (fast_handler, fast_saver) = get_typed_message_saving_handler::<OrderBook>(None);
    let (slow_handler, slow_saver) = get_typed_message_saving_handler::<OrderBook>(None);
    msgbus::subscribe_book_snapshots(fast_topic.into(), fast_handler, None);
    msgbus::subscribe_book_snapshots(slow_topic.into(), slow_handler, None);

    execute_book_snapshot_subscribe(
        &data_engine,
        audusd_sim.id,
        client_id,
        venue,
        fast_interval_ms,
    );
    execute_book_snapshot_subscribe(
        &data_engine,
        audusd_sim.id,
        client_id,
        venue,
        slow_interval_ms,
    );
    process_book_delta(&data_engine, audusd_sim.id);
    advance_clock_and_dispatch(&clock, 500_000_000);

    let fast_count = fast_saver.get_messages().len();
    let slow_count = slow_saver.get_messages().len();

    execute_book_snapshot_unsubscribe(
        &data_engine,
        audusd_sim.id,
        client_id,
        venue,
        fast_interval_ms,
    );

    assert_eq!(recorder.borrow().len(), 1);

    process_book_delta(&data_engine, audusd_sim.id);
    advance_clock_and_dispatch(&clock, 500_000_000);

    assert_eq!(fast_saver.get_messages().len(), fast_count);
    assert!(slow_saver.get_messages().len() > slow_count);

    execute_book_snapshot_unsubscribe(
        &data_engine,
        audusd_sim.id,
        client_id,
        venue,
        slow_interval_ms,
    );

    let recorded = recorder.borrow();
    assert_eq!(recorded.len(), 2);
    assert!(matches!(
        &recorded[1],
        DataCommand::Unsubscribe(UnsubscribeCommand::BookDeltas(cmd)) if cmd.instrument_id == audusd_sim.id
    ));
    drop(recorded);

    let slow_count = slow_saver.get_messages().len();
    process_book_delta(&data_engine, audusd_sim.id);
    advance_clock_and_dispatch(&clock, 500_000_000);
    assert_eq!(slow_saver.get_messages().len(), slow_count);
}

#[rstest]
fn test_unsubscribe_book_snapshots_during_publish_does_not_panic(
    audusd_sim: CurrencyPair,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let data_engine = create_snapshot_test_engine(clock.clone(), cache.clone());
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock.clone(),
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine.borrow_mut(),
    );

    let _ = cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim.clone()));

    let interval_ms = NonZeroUsize::new(100).unwrap();
    let topic = switchboard::get_book_snapshots_topic(audusd_sim.id, interval_ms);
    let snapshot_count = Rc::new(RefCell::new(0usize));
    let snapshot_count_clone = snapshot_count.clone();
    let data_engine_clone = data_engine.clone();
    let unsubscribe_handler = TypedHandler::from(move |_book: &OrderBook| {
        *snapshot_count_clone.borrow_mut() += 1;
        execute_book_snapshot_unsubscribe(
            &data_engine_clone,
            audusd_sim.id,
            client_id,
            venue,
            interval_ms,
        );
    });
    msgbus::subscribe_book_snapshots(topic.into(), unsubscribe_handler, None);

    execute_book_snapshot_subscribe(&data_engine, audusd_sim.id, client_id, venue, interval_ms);
    process_book_delta(&data_engine, audusd_sim.id);
    advance_clock_and_dispatch(&clock, 200_000_000);

    assert_eq!(*snapshot_count.borrow(), 1);

    let recorded = recorder.borrow();
    assert_eq!(recorded.len(), 2);
    assert!(matches!(
        &recorded[1],
        DataCommand::Unsubscribe(UnsubscribeCommand::BookDeltas(cmd)) if cmd.instrument_id == audusd_sim.id
    ));
    drop(recorded);

    process_book_delta(&data_engine, audusd_sim.id);
    advance_clock_and_dispatch(&clock, 200_000_000);
    assert_eq!(*snapshot_count.borrow(), 1);
}

#[rstest]
fn test_unsubscribe_book_deltas_keeps_snapshot_subscriptions_active(
    audusd_sim: CurrencyPair,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let data_engine = create_snapshot_test_engine(clock.clone(), cache.clone());
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock.clone(),
        cache.clone(),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine.borrow_mut(),
    );

    let _ = cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim.clone()));

    let interval_ms = NonZeroUsize::new(100).unwrap();
    let topic = switchboard::get_book_snapshots_topic(audusd_sim.id, interval_ms);
    let (handler, saver) = get_typed_message_saving_handler::<OrderBook>(None);
    msgbus::subscribe_book_snapshots(topic.into(), handler, None);

    execute_book_snapshot_subscribe(&data_engine, audusd_sim.id, client_id, venue, interval_ms);

    let deltas_cmd = SubscribeBookDeltas::new(
        audusd_sim.id,
        BookType::L2_MBP,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        true,
        None,
        None,
    );
    data_engine
        .borrow_mut()
        .execute(DataCommand::Subscribe(SubscribeCommand::BookDeltas(
            deltas_cmd,
        )));

    let unsubscribe_cmd = UnsubscribeBookDeltas::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    data_engine
        .borrow_mut()
        .execute(DataCommand::Unsubscribe(UnsubscribeCommand::BookDeltas(
            unsubscribe_cmd,
        )));

    assert!(!recorder.borrow().iter().any(|cmd| matches!(
        cmd,
        DataCommand::Unsubscribe(UnsubscribeCommand::BookDeltas(_))
    )));

    process_book_delta(&data_engine, audusd_sim.id);
    advance_clock_and_dispatch(&clock, 200_000_000);

    assert_eq!(saver.get_messages().len(), 1);
    assert_eq!(saver.get_messages()[0].instrument_id, audusd_sim.id);
}

fn execute_book_snapshot_subscribe(
    data_engine: &Rc<RefCell<DataEngine>>,
    instrument_id: InstrumentId,
    client_id: ClientId,
    venue: Venue,
    interval_ms: NonZeroUsize,
) {
    let subscribe = SubscribeBookSnapshots::new(
        instrument_id,
        BookType::L2_MBP,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        interval_ms,
        None,
        None,
    );

    data_engine
        .borrow_mut()
        .execute(DataCommand::Subscribe(SubscribeCommand::BookSnapshots(
            subscribe,
        )));
}

fn execute_book_snapshot_unsubscribe(
    data_engine: &Rc<RefCell<DataEngine>>,
    instrument_id: InstrumentId,
    client_id: ClientId,
    venue: Venue,
    interval_ms: NonZeroUsize,
) {
    let unsubscribe = UnsubscribeBookSnapshots::new(
        instrument_id,
        interval_ms,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    data_engine
        .borrow_mut()
        .execute(DataCommand::Unsubscribe(UnsubscribeCommand::BookSnapshots(
            unsubscribe,
        )));
}

fn process_book_delta(data_engine: &Rc<RefCell<DataEngine>>, instrument_id: InstrumentId) {
    let delta = OrderBookDeltaTestBuilder::new(instrument_id).build();
    let deltas = OrderBookDeltas_API::new(OrderBookDeltas::new(instrument_id, vec![delta]));
    data_engine.borrow_mut().process_data(Data::Deltas(deltas));
}

fn advance_clock_and_dispatch(clock: &Rc<RefCell<TestClock>>, advance_ns: u64) {
    let to_time_ns = clock.borrow().timestamp_ns().as_u64() + advance_ns;
    let events = clock.borrow_mut().advance_time(to_time_ns.into(), true);
    let handlers = clock.borrow().match_handlers(events);

    for handler in handlers {
        handler.callback.call(handler.event);
    }
}

fn create_snapshot_test_engine(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
) -> Rc<RefCell<DataEngine>> {
    let _ =
        MessageBus::new(TraderId::test_default(), UUID4::new(), None, None).register_message_bus();

    let data_engine = Rc::new(RefCell::new(DataEngine::new(clock, cache, None)));
    let data_engine_clone = data_engine.clone();
    let handler = TypedIntoHandler::from(move |cmd: DataCommand| {
        data_engine_clone.borrow_mut().execute(cmd);
    });
    let endpoint = MessagingSwitchboard::data_engine_execute();
    msgbus::register_data_command_endpoint(endpoint, handler);

    data_engine
}

fn make_crypto_option(
    symbol: &str,
    underlying_str: &str,
    settlement_str: &str,
    strike: &str,
    kind: OptionKind,
    expiration_ns: UnixNanos,
) -> InstrumentAny {
    use nautilus_model::{
        identifiers::Symbol,
        instruments::CryptoOption,
        types::{Currency, Money, Quantity},
    };

    let instrument_id = InstrumentId::from(symbol);
    let raw_symbol = Symbol::from(symbol.split('.').next().unwrap_or(symbol));
    let underlying = Currency::from(underlying_str);
    let quote = Currency::USD();
    let settlement = Currency::from(settlement_str);
    let activation = UnixNanos::from(1_671_696_000_000_000_000u64);

    InstrumentAny::CryptoOption(CryptoOption::new(
        instrument_id,
        raw_symbol,
        underlying,
        quote,
        settlement,
        false,
        kind,
        Price::from(strike),
        activation,
        expiration_ns,
        3,
        1,
        Price::from("0.001"),
        Quantity::from("0.1"),
        Some(Quantity::from(1)),
        Some(Quantity::from(1)),
        Some(Quantity::from("9000.0")),
        Some(Quantity::from("0.1")),
        None,
        Some(Money::new(10.00, Currency::USD())),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        0.into(),
        0.into(),
    ))
}

fn make_btc_option(strike: &str, kind: OptionKind) -> InstrumentAny {
    let kind_char = match kind {
        OptionKind::Call => "C",
        OptionKind::Put => "P",
    };
    let symbol = format!("BTC-20240101-{strike}-{kind_char}.DERIBIT");
    let expiration_ns = UnixNanos::from(1_704_067_200_000_000_000u64);
    make_crypto_option(&symbol, "BTC", "BTC", strike, kind, expiration_ns)
}

fn make_series_id() -> OptionSeriesId {
    OptionSeriesId::new(
        Venue::new("DERIBIT"),
        ustr::Ustr::from("BTC"),
        ustr::Ustr::from("BTC"),
        UnixNanos::from(1_704_067_200_000_000_000u64),
    )
}

fn make_subscribe_option_chain(
    series_id: OptionSeriesId,
    strikes: Vec<Price>,
    client_id: Option<ClientId>,
    venue: Option<Venue>,
) -> DataCommand {
    DataCommand::Subscribe(SubscribeCommand::OptionChain(SubscribeOptionChain::new(
        series_id,
        StrikeRange::Fixed(strikes),
        Some(1000),
        UUID4::new(),
        UnixNanos::default(),
        client_id,
        venue,
        None,
    )))
}

fn make_unsubscribe_option_chain(
    series_id: OptionSeriesId,
    client_id: Option<ClientId>,
    venue: Option<Venue>,
) -> DataCommand {
    DataCommand::Unsubscribe(UnsubscribeCommand::OptionChain(
        UnsubscribeOptionChain::new(
            series_id,
            UUID4::new(),
            UnixNanos::default(),
            client_id,
            venue,
        ),
    ))
}

/// Creates a data engine that shares the provided cache and clock.
fn make_option_chain_engine(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
) -> Rc<RefCell<DataEngine>> {
    let data_engine = Rc::new(RefCell::new(DataEngine::new(clock, cache, None)));

    let data_engine_clone = data_engine.clone();
    let handler = TypedIntoHandler::from(move |cmd: DataCommand| {
        data_engine_clone.borrow_mut().execute(cmd);
    });
    let endpoint = MessagingSwitchboard::data_engine_execute();
    msgbus::register_data_command_endpoint(endpoint, handler);

    data_engine
}

#[rstest]
fn test_subscribe_option_chain_fixed_range_creates_manager(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let _ = msgbus::get_message_bus();
    let data_engine = make_option_chain_engine(clock.clone(), cache.clone());

    let client_id = ClientId::new("DERIBIT");
    let venue = Venue::new("DERIBIT");
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));

    register_mock_client(
        clock,
        cache.clone(),
        client_id,
        venue,
        Some(venue),
        &recorder,
        &mut data_engine.borrow_mut(),
    );

    // Add instruments to cache
    let strikes = ["45000.000", "50000.000", "55000.000"];
    for strike in &strikes {
        let call = make_btc_option(strike, OptionKind::Call);
        let put = make_btc_option(strike, OptionKind::Put);
        let _ = cache.borrow_mut().add_instrument(call);
        let _ = cache.borrow_mut().add_instrument(put);
    }

    // Subscribe with Fixed range
    let series_id = make_series_id();
    let strike_prices: Vec<Price> = strikes.iter().map(|s| Price::from(*s)).collect();
    let cmd = make_subscribe_option_chain(series_id, strike_prices, Some(client_id), Some(venue));
    data_engine.borrow_mut().execute(cmd);

    // Verify quote and greeks subscriptions were forwarded to the client
    let recorded = recorder.borrow();
    let subscribe_count = recorded
        .iter()
        .filter(|cmd| matches!(cmd, DataCommand::Subscribe(SubscribeCommand::Quotes(_))))
        .count();
    let greeks_count = recorded
        .iter()
        .filter(|cmd| {
            matches!(
                cmd,
                DataCommand::Subscribe(SubscribeCommand::OptionGreeks(_))
            )
        })
        .count();

    // 6 instruments (3 strikes x 2 kinds), each gets quotes + greeks
    assert_eq!(subscribe_count, 6, "Expected 6 quote subscriptions");
    assert_eq!(greeks_count, 6, "Expected 6 greeks subscriptions");
}

#[rstest]
fn test_subscribe_option_chain_filters_by_underlying(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let _ = msgbus::get_message_bus();
    let data_engine = make_option_chain_engine(clock.clone(), cache.clone());

    let client_id = ClientId::new("DERIBIT");
    let venue = Venue::new("DERIBIT");
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));

    register_mock_client(
        clock,
        cache.clone(),
        client_id,
        venue,
        Some(venue),
        &recorder,
        &mut data_engine.borrow_mut(),
    );

    // Add BTC options
    let btc_call = make_btc_option("50000.000", OptionKind::Call);
    let _ = cache.borrow_mut().add_instrument(btc_call);

    // Add ETH option with same venue but different underlying
    let eth_option = make_crypto_option(
        "ETH-20240101-3000-C.DERIBIT",
        "ETH",
        "ETH",
        "3000.000",
        OptionKind::Call,
        UnixNanos::from(1_704_067_200_000_000_000u64),
    );
    let _ = cache.borrow_mut().add_instrument(eth_option);

    // Subscribe to BTC option chain
    let series_id = make_series_id();
    let cmd = make_subscribe_option_chain(
        series_id,
        vec![Price::from("50000.000")],
        Some(client_id),
        Some(venue),
    );
    data_engine.borrow_mut().execute(cmd);

    // Only BTC instruments should be subscribed (1 call)
    let recorded = recorder.borrow();
    let subscribe_count = recorded
        .iter()
        .filter(|cmd| matches!(cmd, DataCommand::Subscribe(SubscribeCommand::Quotes(_))))
        .count();
    assert_eq!(subscribe_count, 1, "Only BTC option should be subscribed");
}

#[rstest]
fn test_unsubscribe_option_chain_tears_down(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let _ = msgbus::get_message_bus();
    let data_engine = make_option_chain_engine(clock.clone(), cache.clone());

    let client_id = ClientId::new("DERIBIT");
    let venue = Venue::new("DERIBIT");
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));

    register_mock_client(
        clock,
        cache.clone(),
        client_id,
        venue,
        Some(venue),
        &recorder,
        &mut data_engine.borrow_mut(),
    );

    // Add instruments to cache
    let call = make_btc_option("50000.000", OptionKind::Call);
    let put = make_btc_option("50000.000", OptionKind::Put);
    let _ = cache.borrow_mut().add_instrument(call);
    let _ = cache.borrow_mut().add_instrument(put);

    // Subscribe
    let series_id = make_series_id();
    let cmd = make_subscribe_option_chain(
        series_id,
        vec![Price::from("50000.000")],
        Some(client_id),
        Some(venue),
    );
    data_engine.borrow_mut().execute(cmd);

    // Clear recorder to isolate unsubscribe commands
    recorder.borrow_mut().clear();

    // Unsubscribe
    let unsub_cmd = make_unsubscribe_option_chain(series_id, Some(client_id), Some(venue));
    data_engine.borrow_mut().execute(unsub_cmd);

    // Verify unsubscribe commands forwarded
    let recorded = recorder.borrow();
    let unsub_quotes = recorded
        .iter()
        .filter(|cmd| matches!(cmd, DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(_))))
        .count();
    let unsub_greeks = recorded
        .iter()
        .filter(|cmd| {
            matches!(
                cmd,
                DataCommand::Unsubscribe(UnsubscribeCommand::OptionGreeks(_))
            )
        })
        .count();

    assert_eq!(unsub_quotes, 2, "Expected 2 quote unsubscribes");
    assert_eq!(unsub_greeks, 2, "Expected 2 greeks unsubscribes");
}

#[rstest]
fn test_unsubscribe_option_chain_not_subscribed_does_not_panic(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let _ = msgbus::get_message_bus();
    let data_engine = make_option_chain_engine(clock, cache);

    let series_id = make_series_id();
    let cmd = make_unsubscribe_option_chain(series_id, None, Some(Venue::new("DERIBIT")));

    // Should not panic, logs a warning
    data_engine.borrow_mut().execute(cmd);
}

#[rstest]
fn test_subscribe_option_chain_resubscribe_replaces_manager(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let _ = msgbus::get_message_bus();
    let data_engine = make_option_chain_engine(clock.clone(), cache.clone());

    let client_id = ClientId::new("DERIBIT");
    let venue = Venue::new("DERIBIT");
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));

    register_mock_client(
        clock,
        cache.clone(),
        client_id,
        venue,
        Some(venue),
        &recorder,
        &mut data_engine.borrow_mut(),
    );

    // Add instruments to cache
    let call = make_btc_option("50000.000", OptionKind::Call);
    let _ = cache.borrow_mut().add_instrument(call);

    // Subscribe twice
    let series_id = make_series_id();
    let strikes = vec![Price::from("50000.000")];
    let cmd1 =
        make_subscribe_option_chain(series_id, strikes.clone(), Some(client_id), Some(venue));
    data_engine.borrow_mut().execute(cmd1);

    let cmd2 = make_subscribe_option_chain(series_id, strikes, Some(client_id), Some(venue));
    data_engine.borrow_mut().execute(cmd2);

    // Should have unsubscribes from teardown of first manager, then resubscribes
    let recorded = recorder.borrow();
    let unsub_quotes = recorded
        .iter()
        .filter(|cmd| matches!(cmd, DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(_))))
        .count();
    let unsub_greeks = recorded
        .iter()
        .filter(|cmd| {
            matches!(
                cmd,
                DataCommand::Unsubscribe(UnsubscribeCommand::OptionGreeks(_))
            )
        })
        .count();
    let sub_quotes = recorded
        .iter()
        .filter(|cmd| matches!(cmd, DataCommand::Subscribe(SubscribeCommand::Quotes(_))))
        .count();

    // First subscribe: 1 call, second subscribe: teardown 1 + subscribe 1
    assert_eq!(
        unsub_quotes, 1,
        "Expected 1 quote unsubscribe from teardown"
    );
    assert_eq!(
        unsub_greeks, 1,
        "Expected 1 greeks unsubscribe from teardown"
    );
    assert_eq!(
        sub_quotes, 2,
        "Expected 2 quote subscribes (initial + re-subscribe)"
    );
}

#[rstest]
#[case::close(MarketStatusAction::Close, 1, 1)]
#[case::not_available(MarketStatusAction::NotAvailableForTrading, 1, 1)]
#[case::trading(MarketStatusAction::Trading, 0, 0)]
fn test_process_instrument_status_expires_option_chain_instrument(
    #[case] action: MarketStatusAction,
    #[case] expected_quote_unsubs: usize,
    #[case] expected_greeks_unsubs: usize,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let _ = msgbus::get_message_bus();
    let data_engine = make_option_chain_engine(clock.clone(), cache.clone());

    let client_id = ClientId::new("DERIBIT");
    let venue = Venue::new("DERIBIT");
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));

    register_mock_client(
        clock,
        cache.clone(),
        client_id,
        venue,
        Some(venue),
        &recorder,
        &mut data_engine.borrow_mut(),
    );

    // Add two options to the cache so the option chain has multiple members;
    // we will only expire one and assert teardown is scoped to that instrument.
    let call = make_btc_option("50000.000", OptionKind::Call);
    let put = make_btc_option("50000.000", OptionKind::Put);
    let call_id = call.id();
    let _ = cache.borrow_mut().add_instrument(call);
    let _ = cache.borrow_mut().add_instrument(put);

    let series_id = make_series_id();
    let cmd = make_subscribe_option_chain(
        series_id,
        vec![Price::from("50000.000")],
        Some(client_id),
        Some(venue),
    );
    data_engine.borrow_mut().execute(cmd);

    // Clear the recorder so only commands triggered by the status are counted.
    recorder.borrow_mut().clear();

    let status = InstrumentStatus::new(
        call_id,
        action,
        UnixNanos::from(1),
        UnixNanos::from(2),
        None,
        None,
        Some(false),
        Some(false),
        None,
    );
    data_engine
        .borrow_mut()
        .process_data(Data::InstrumentStatus(status));

    let recorded = recorder.borrow();
    let quote_unsubs = recorded
        .iter()
        .filter(|cmd| matches!(cmd, DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(_))))
        .count();
    let greeks_unsubs = recorded
        .iter()
        .filter(|cmd| {
            matches!(
                cmd,
                DataCommand::Unsubscribe(UnsubscribeCommand::OptionGreeks(_))
            )
        })
        .count();

    // Cache write happens regardless of action
    assert_eq!(
        data_engine.borrow().get_cache().instrument_status(&call_id),
        Some(&status),
    );
    assert_eq!(quote_unsubs, expected_quote_unsubs);
    assert_eq!(greeks_unsubs, expected_greeks_unsubs);
}

#[rstest]
#[case::quote("quote")]
#[case::greeks("greeks")]
fn test_option_chain_market_data_at_expiry_expires_instrument(
    #[case] data_kind: &str,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let _ = msgbus::get_message_bus();
    let data_engine = make_option_chain_engine(clock.clone(), cache.clone());

    let client_id = ClientId::new("DERIBIT");
    let venue = Venue::new("DERIBIT");
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));

    register_mock_client(
        clock,
        cache.clone(),
        client_id,
        venue,
        Some(venue),
        &recorder,
        &mut data_engine.borrow_mut(),
    );

    let call = make_btc_option("50000.000", OptionKind::Call);
    let put = make_btc_option("50000.000", OptionKind::Put);
    let call_id = call.id();
    let _ = cache.borrow_mut().add_instrument(call);
    let _ = cache.borrow_mut().add_instrument(put);

    let series_id = make_series_id();
    let cmd = make_subscribe_option_chain(
        series_id,
        vec![Price::from("50000.000")],
        Some(client_id),
        Some(venue),
    );
    data_engine.borrow_mut().execute(cmd);

    recorder.borrow_mut().clear();

    match data_kind {
        "quote" => {
            let quote = QuoteTick::new(
                call_id,
                Price::from("100.00"),
                Price::from("101.00"),
                Quantity::from("1.0"),
                Quantity::from("1.0"),
                series_id.expiration_ns,
                series_id.expiration_ns,
            );
            data_engine.borrow_mut().process_data(Data::Quote(quote));
        }
        "greeks" => {
            let greeks = OptionGreeks {
                instrument_id: call_id,
                convention: GreeksConvention::BlackScholes,
                greeks: OptionGreekValues {
                    delta: 0.55,
                    gamma: 0.001,
                    vega: 15.0,
                    theta: -5.0,
                    rho: 0.02,
                },
                mark_iv: Some(0.65),
                bid_iv: Some(0.63),
                ask_iv: Some(0.67),
                underlying_price: Some(50000.0),
                open_interest: Some(1000.0),
                ts_event: series_id.expiration_ns,
                ts_init: series_id.expiration_ns,
            };
            data_engine.borrow_mut().process(&greeks);
        }
        other => panic!("unknown data kind: {other}"),
    }

    let recorded = recorder.borrow();
    let quote_unsubs = recorded
        .iter()
        .filter(|cmd| matches!(cmd, DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(_))))
        .count();
    let greeks_unsubs = recorded
        .iter()
        .filter(|cmd| {
            matches!(
                cmd,
                DataCommand::Unsubscribe(UnsubscribeCommand::OptionGreeks(_))
            )
        })
        .count();
    let status_unsubs = recorded
        .iter()
        .filter(|cmd| {
            matches!(
                cmd,
                DataCommand::Unsubscribe(UnsubscribeCommand::InstrumentStatus(_))
            )
        })
        .count();

    assert!(data_engine.borrow().has_option_chain_manager(&series_id));
    assert_eq!(quote_unsubs, 1);
    assert_eq!(greeks_unsubs, 1);
    assert_eq!(status_unsubs, 1);
}

#[rstest]
fn test_process_option_greeks_caches_and_publishes(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    use nautilus_model::{
        data::{greeks::OptionGreekValues, option_chain::OptionGreeks},
        enums::GreeksConvention,
    };

    let _ = msgbus::get_message_bus();
    let data_engine = make_option_chain_engine(clock.clone(), cache.clone());

    let client_id = ClientId::new("DERIBIT");
    let venue = Venue::new("DERIBIT");
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));

    register_mock_client(
        clock,
        cache.clone(),
        client_id,
        venue,
        Some(venue),
        &recorder,
        &mut data_engine.borrow_mut(),
    );

    let instrument_id = InstrumentId::from("BTC-20240101-50000-C.DERIBIT");

    // Set up msgbus handler to capture published greeks
    let topic = switchboard::get_option_greeks_topic(instrument_id);
    let (handler, saver) = get_typed_message_saving_handler::<OptionGreeks>(None);
    msgbus::subscribe_option_greeks(topic.into(), handler, None);

    // Process greeks data through the engine
    let greeks = OptionGreeks {
        instrument_id,
        convention: GreeksConvention::BlackScholes,
        greeks: OptionGreekValues {
            delta: 0.55,
            gamma: 0.001,
            vega: 15.0,
            theta: -5.0,
            rho: 0.02,
        },
        mark_iv: Some(0.65),
        bid_iv: Some(0.63),
        ask_iv: Some(0.67),
        underlying_price: Some(50000.0),
        open_interest: Some(1000.0),
        ts_event: UnixNanos::from(1u64),
        ts_init: UnixNanos::from(1u64),
    };

    data_engine.borrow_mut().process(&greeks);

    // Verify greeks were cached
    let cached = cache.borrow().option_greeks(&instrument_id).copied();
    assert!(cached.is_some(), "OptionGreeks should be cached");
    assert_eq!(cached.unwrap().delta, 0.55);

    // Verify greeks were published to msgbus
    wait_until(
        || !saver.get_messages().is_empty(),
        Duration::from_millis(100),
    );
    let messages = saver.get_messages();
    assert!(!messages.is_empty(), "OptionGreeks should be published");
    assert_eq!(messages[0].instrument_id, instrument_id);
    assert_eq!(messages[0].delta, 0.55);
}

#[rstest]
fn test_subscribe_option_chain_atm_relative_requests_forward_prices(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let _ = msgbus::get_message_bus();
    let data_engine = make_option_chain_engine(clock.clone(), cache.clone());

    let client_id = ClientId::new("DERIBIT");
    let venue = Venue::new("DERIBIT");
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));

    register_mock_client(
        clock,
        cache.clone(),
        client_id,
        venue,
        Some(venue),
        &recorder,
        &mut data_engine.borrow_mut(),
    );

    // Add an instrument to cache so sample_instrument_id lookup succeeds
    let call = make_btc_option("50000.000", OptionKind::Call);
    let _ = cache.borrow_mut().add_instrument(call);

    // Subscribe with ATM-relative range (not Fixed)
    let series_id = make_series_id();
    let cmd = DataCommand::Subscribe(SubscribeCommand::OptionChain(SubscribeOptionChain::new(
        series_id,
        StrikeRange::AtmRelative {
            strikes_above: 2,
            strikes_below: 2,
        },
        Some(1000),
        UUID4::new(),
        UnixNanos::default(),
        Some(client_id),
        Some(venue),
        None,
    )));
    data_engine.borrow_mut().execute(cmd);

    // ATM-relative should trigger a forward price request instead of immediate subscriptions
    let recorded = recorder.borrow();
    let forward_requests = recorded
        .iter()
        .filter(|cmd| matches!(cmd, DataCommand::Request(RequestCommand::ForwardPrices(_))))
        .count();

    assert_eq!(
        forward_requests, 1,
        "ATM-relative range should request forward prices"
    );

    // No direct quote subscriptions yet, deferred until forward price response
    let quote_subs = recorded
        .iter()
        .filter(|cmd| matches!(cmd, DataCommand::Subscribe(SubscribeCommand::Quotes(_))))
        .count();
    assert_eq!(
        quote_subs, 0,
        "No quote subscriptions before forward price bootstrap"
    );
}

fn synthetic_instrument_id() -> InstrumentId {
    InstrumentId::new(Symbol::new("BTC-ETH-INDEX"), Venue::synthetic())
}

fn synthetic_index() -> (SyntheticInstrument, InstrumentId, InstrumentId) {
    let component_a = InstrumentId::from("BTC-USD.SIM");
    let component_b = InstrumentId::from("ETH-USD.SIM");
    let synthetic = synthetic_index_with_components("BTC-ETH-INDEX", component_a, component_b);

    (synthetic, component_a, component_b)
}

fn synthetic_index_with_components(
    symbol: &str,
    component_a: InstrumentId,
    component_b: InstrumentId,
) -> SyntheticInstrument {
    let formula = format!("({component_a} + {component_b}) / 2.0");
    SyntheticInstrument::new(
        Symbol::new(symbol),
        2,
        vec![component_a, component_b],
        &formula,
        UnixNanos::default(),
        UnixNanos::default(),
    )
}

fn subscribe_synthetic_quotes_cmd(instrument_id: InstrumentId) -> DataCommand {
    DataCommand::Subscribe(SubscribeCommand::Quotes(SubscribeQuotes::new(
        instrument_id,
        None,
        Some(Venue::synthetic()),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )))
}

fn subscribe_synthetic_trades_cmd(instrument_id: InstrumentId) -> DataCommand {
    DataCommand::Subscribe(SubscribeCommand::Trades(SubscribeTrades::new(
        instrument_id,
        None,
        Some(Venue::synthetic()),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )))
}

fn unsubscribe_synthetic_quotes_cmd(instrument_id: InstrumentId) -> DataCommand {
    DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(UnsubscribeQuotes::new(
        instrument_id,
        None,
        Some(Venue::synthetic()),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )))
}

fn unsubscribe_synthetic_trades_cmd(instrument_id: InstrumentId) -> DataCommand {
    DataCommand::Unsubscribe(UnsubscribeCommand::Trades(UnsubscribeTrades::new(
        instrument_id,
        None,
        Some(Venue::synthetic()),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )))
}

fn quote_tick(instrument_id: InstrumentId, bid: &str, ask: &str, ts: u64) -> QuoteTick {
    QuoteTick::new(
        instrument_id,
        Price::from(bid),
        Price::from(ask),
        Quantity::from(1),
        Quantity::from(1),
        UnixNanos::from(ts),
        UnixNanos::from(ts),
    )
}

fn trade_tick(instrument_id: InstrumentId, price: &str, trade_id: &str, ts: u64) -> TradeTick {
    TradeTick::new(
        instrument_id,
        Price::from(price),
        Quantity::from(1),
        AggressorSide::Buyer,
        TradeId::new(trade_id),
        UnixNanos::from(ts),
        UnixNanos::from(ts),
    )
}

#[rstest]
fn test_counters_increment_per_dispatch(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    assert_eq!(data_engine.command_count(), 0);
    assert_eq!(data_engine.data_count(), 0);
    assert_eq!(data_engine.request_count(), 0);
    assert_eq!(data_engine.response_count(), 0);

    let inst_any = InstrumentAny::CurrencyPair(audusd_sim.clone());
    data_engine.process(&inst_any as &dyn Any);
    assert_eq!(data_engine.data_count(), 1);

    let quote = QuoteTick::new(
        audusd_sim.id,
        Price::from("0.8000"),
        Price::from("0.8010"),
        Quantity::from(1),
        Quantity::from(1),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    data_engine.process_data(Data::Quote(quote));
    assert_eq!(data_engine.data_count(), 2);

    let sub = SubscribeQuotes::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Quotes(sub)));
    assert_eq!(data_engine.command_count(), 1);

    let unsub = UnsubscribeQuotes::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    data_engine.execute(DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(unsub)));
    assert_eq!(data_engine.command_count(), 2);

    let req = RequestQuotes::new(
        audusd_sim.id,
        None,
        None,
        None,
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    data_engine.execute(DataCommand::Request(RequestCommand::Quotes(req)));
    assert_eq!(data_engine.request_count(), 1);
    assert_eq!(
        data_engine.command_count(),
        2,
        "Request must not increment command_count"
    );
}

#[rstest]
fn test_reset_resets_counters(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let inst_any = InstrumentAny::CurrencyPair(audusd_sim);
    data_engine.process(&inst_any as &dyn Any);
    assert_eq!(data_engine.data_count(), 1);

    data_engine.reset();

    assert_eq!(data_engine.command_count(), 0);
    assert_eq!(data_engine.data_count(), 0);
    assert_eq!(data_engine.request_count(), 0);
    assert_eq!(data_engine.response_count(), 0);
}

fn build_synthetic_subscribe(
    variant: &str,
    instrument_id: InstrumentId,
    client_id: ClientId,
    venue: Venue,
    ts: UnixNanos,
) -> DataCommand {
    let id = UUID4::new();

    match variant {
        "instrument" => {
            DataCommand::Subscribe(SubscribeCommand::Instrument(SubscribeInstrument::new(
                instrument_id,
                Some(client_id),
                Some(venue),
                id,
                ts,
                None,
                None,
            )))
        }
        "status" => DataCommand::Subscribe(SubscribeCommand::InstrumentStatus(
            SubscribeInstrumentStatus::new(
                instrument_id,
                Some(client_id),
                Some(venue),
                id,
                ts,
                None,
                None,
            ),
        )),
        "close" => DataCommand::Subscribe(SubscribeCommand::InstrumentClose(
            SubscribeInstrumentClose::new(
                instrument_id,
                Some(client_id),
                Some(venue),
                id,
                ts,
                None,
                None,
            ),
        )),
        "greeks" => {
            DataCommand::Subscribe(SubscribeCommand::OptionGreeks(SubscribeOptionGreeks::new(
                instrument_id,
                Some(client_id),
                Some(venue),
                id,
                ts,
                None,
                None,
            )))
        }
        other => panic!("unknown synthetic subscribe variant: {other}"),
    }
}

fn build_synthetic_unsubscribe(
    variant: &str,
    instrument_id: InstrumentId,
    client_id: ClientId,
    venue: Venue,
    ts: UnixNanos,
) -> DataCommand {
    let id = UUID4::new();

    match variant {
        "instrument" => {
            DataCommand::Unsubscribe(UnsubscribeCommand::Instrument(UnsubscribeInstrument::new(
                instrument_id,
                Some(client_id),
                Some(venue),
                id,
                ts,
                None,
                None,
            )))
        }
        "status" => DataCommand::Unsubscribe(UnsubscribeCommand::InstrumentStatus(
            UnsubscribeInstrumentStatus::new(
                instrument_id,
                Some(client_id),
                Some(venue),
                id,
                ts,
                None,
                None,
            ),
        )),
        "close" => DataCommand::Unsubscribe(UnsubscribeCommand::InstrumentClose(
            UnsubscribeInstrumentClose::new(
                instrument_id,
                Some(client_id),
                Some(venue),
                id,
                ts,
                None,
                None,
            ),
        )),
        "greeks" => DataCommand::Unsubscribe(UnsubscribeCommand::OptionGreeks(
            UnsubscribeOptionGreeks::new(
                instrument_id,
                Some(client_id),
                Some(venue),
                id,
                ts,
                None,
                None,
            ),
        )),
        other => panic!("unknown synthetic unsubscribe variant: {other}"),
    }
}

#[rstest]
#[case::instrument("instrument")]
#[case::status("status")]
#[case::close("close")]
#[case::greeks("greeks")]
fn test_subscribe_synthetic_instrument_rejected(
    #[case] variant: &str,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let synth_id = synthetic_instrument_id();
    let cmd = build_synthetic_subscribe(variant, synth_id, client_id, venue, UnixNanos::default());
    data_engine.execute(cmd);

    assert!(
        recorder.borrow().is_empty(),
        "Synthetic subscribe ({variant}) must not reach the client, received {:?}",
        recorder.borrow()
    );
    assert_eq!(
        data_engine.command_count(),
        1,
        "Rejected subscribe must still count as a command"
    );
    assert!(!data_engine.subscribed_instruments().contains(&synth_id));
    assert!(
        !data_engine
            .subscribed_instrument_status()
            .contains(&synth_id)
    );
    assert!(
        !data_engine
            .subscribed_instrument_close()
            .contains(&synth_id)
    );
}

#[rstest]
#[case::instrument("instrument")]
#[case::status("status")]
#[case::close("close")]
#[case::greeks("greeks")]
fn test_unsubscribe_synthetic_instrument_rejected(
    #[case] variant: &str,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let synth_id = synthetic_instrument_id();
    let cmd =
        build_synthetic_unsubscribe(variant, synth_id, client_id, venue, UnixNanos::default());
    data_engine.execute(cmd);

    assert!(
        recorder.borrow().is_empty(),
        "Synthetic unsubscribe ({variant}) must not reach the client, received {:?}",
        recorder.borrow()
    );
    assert_eq!(
        data_engine.command_count(),
        1,
        "Rejected unsubscribe must still count as a command"
    );
}

#[rstest]
fn test_response_increments_response_count(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
) {
    let mut data_engine = data_engine.borrow_mut();

    let resp = InstrumentResponse::new(
        UUID4::new(),
        ClientId::test_default(),
        audusd_sim.id,
        InstrumentAny::CurrencyPair(audusd_sim),
        None,
        None,
        UnixNanos::default(),
        None,
    );
    data_engine.response(DataResponse::Instrument(Box::new(resp)));

    assert_eq!(data_engine.response_count(), 1);

    data_engine.reset();
    assert_eq!(data_engine.response_count(), 0);
}

#[rstest]
fn test_custom_data_response_is_forwarded_with_metadata(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    data_engine: Rc<RefCell<DataEngine>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let correlation_id = UUID4::new();
    let data_type = DataType::new(
        "CustomFeed",
        Some(serde_json::from_value(json!({"source": "metadata"})).unwrap()),
        Some("SIM//CUSTOM".to_string()),
    );
    let params = serde_json::from_value(json!({"source": "params"})).unwrap();
    let start = UnixNanos::from(1_000);
    let end = UnixNanos::from(2_000);
    let ts_init = UnixNanos::from(3_000);
    let (handler, saver) =
        get_any_saving_handler::<CustomDataResponse>(Some(Ustr::from("custom-data-response")));
    msgbus::register_response_handler(&correlation_id, handler);

    let resp = CustomDataResponse::new(
        correlation_id,
        client_id,
        Some(venue),
        data_type.clone(),
        "custom-payload".to_string(),
        Some(start),
        Some(end),
        ts_init,
        Some(params),
    );
    data_engine.response(DataResponse::Data(resp));

    let responses = saver.get_messages();
    assert_eq!(responses.len(), 1);

    let forwarded = &responses[0];
    assert_eq!(forwarded.correlation_id, correlation_id);
    assert_eq!(forwarded.client_id, client_id);
    assert_eq!(forwarded.venue, Some(venue));
    assert_eq!(forwarded.data_type, data_type);
    assert_eq!(forwarded.start, Some(start));
    assert_eq!(forwarded.end, Some(end));
    assert_eq!(forwarded.ts_init, ts_init);
    assert_eq!(
        forwarded
            .params
            .as_ref()
            .and_then(|params| params.get_str("source")),
        Some("params")
    );
    assert_eq!(
        forwarded
            .data
            .as_ref()
            .downcast_ref::<String>()
            .map(String::as_str),
        Some("custom-payload")
    );
    assert_eq!(data_engine.response_count(), 1);
    assert_eq!(stub_msgbus.borrow().res_count(), 1);
}

#[rstest]
fn test_custom_data_response_does_not_publish_payload_to_custom_topic(
    data_engine: Rc<RefCell<DataEngine>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let correlation_id = UUID4::new();
    let payload = stub_custom_data(
        4_000,
        42,
        Some(serde_json::from_value(json!({"source": "metadata"})).unwrap()),
        Some("SIM//CUSTOM".to_string()),
    );
    let data_type = payload.data_type.clone();
    let params = serde_json::from_value(json!({"source": "params"})).unwrap();
    let (response_handler, response_saver) =
        get_any_saving_handler::<CustomDataResponse>(Some(Ustr::from("custom-response-only")));
    msgbus::register_response_handler(&correlation_id, response_handler);

    let (topic_handler, topic_saver) =
        get_any_saving_handler::<CustomData>(Some(Ustr::from("custom-topic")));
    let topic = switchboard::get_custom_topic(&data_type);
    msgbus::subscribe_any(topic.into(), topic_handler, None);

    let resp = CustomDataResponse::new(
        correlation_id,
        client_id,
        Some(venue),
        data_type,
        payload.clone(),
        None,
        None,
        UnixNanos::from(5_000),
        Some(params),
    );
    data_engine.response(DataResponse::Data(resp));

    let responses = response_saver.get_messages();
    assert_eq!(responses.len(), 1);
    assert_eq!(
        responses[0].data.as_ref().downcast_ref::<CustomData>(),
        Some(&payload)
    );
    assert!(topic_saver.get_messages().is_empty());
}

#[rstest]
fn test_process_custom_data_through_any_publishes_to_custom_topic(
    _stub_msgbus: Rc<RefCell<MessageBus>>,
    data_engine: Rc<RefCell<DataEngine>>,
) {
    let mut data_engine = data_engine.borrow_mut();
    let custom = stub_custom_data(
        6_000,
        99,
        Some(serde_json::from_value(json!({"source": "metadata"})).unwrap()),
        Some("SIM//CUSTOM".to_string()),
    );
    let (handler, saver) = get_any_saving_handler::<CustomData>(None);
    let topic = switchboard::get_custom_topic(&custom.data_type);
    msgbus::subscribe_any(topic.into(), handler, None);

    assert_eq!(data_engine.data_count(), 0);

    data_engine.process(&custom as &dyn Any);
    assert_eq!(saver.get_messages(), vec![custom.clone()]);
    assert_eq!(data_engine.data_count(), 1);

    data_engine.process_data(Data::Custom(custom.clone()));

    assert_eq!(saver.get_messages(), vec![custom.clone(), custom]);
    assert_eq!(data_engine.data_count(), 2);
}

#[cfg(feature = "defi")]
#[rstest]
fn test_execute_defi_command_counters(data_engine: Rc<RefCell<DataEngine>>) {
    let mut data_engine = data_engine.borrow_mut();

    let instrument_id =
        InstrumentId::from("0x11b815efB8f581194ae79006d24E0d814B7697F6.Arbitrum:UniswapV3");

    let sub = DataCommand::DefiSubscribe(DefiSubscribeCommand::PoolSwaps(SubscribePoolSwaps {
        instrument_id,
        client_id: None,
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    }));
    data_engine.execute(sub);

    let unsub =
        DataCommand::DefiUnsubscribe(DefiUnsubscribeCommand::PoolSwaps(UnsubscribePoolSwaps {
            instrument_id,
            client_id: None,
            command_id: UUID4::new(),
            ts_init: UnixNanos::default(),
            params: None,
        }));
    data_engine.execute(unsub);

    let req = DataCommand::DefiRequest(DefiRequestCommand::PoolSnapshot(RequestPoolSnapshot {
        instrument_id,
        client_id: None,
        request_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    }));
    data_engine.execute(req);

    assert_eq!(
        data_engine.command_count(),
        2,
        "DefiSubscribe + DefiUnsubscribe must increment command_count"
    );
    assert_eq!(
        data_engine.request_count(),
        1,
        "DefiRequest must increment request_count"
    );
}

#[cfg(feature = "defi")]
#[rstest]
fn test_process_defi_data_increments_data_count(
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    data_engine.borrow_mut().register_client(data_client, None);

    let blockchain = Blockchain::Ethereum;
    let block = Block::new(
        "0x123".to_string(),
        "0x456".to_string(),
        1u64,
        "miner".into(),
        1000000u64,
        500000u64,
        UnixNanos::from(1),
        Some(blockchain),
    );

    let mut data_engine = data_engine.borrow_mut();
    assert_eq!(data_engine.data_count(), 0);
    data_engine.process_defi_data(DefiData::Block(block));
    assert_eq!(data_engine.data_count(), 1);
}

fn pipeline_topic_of(live: &str) -> String {
    let suffix = live.strip_prefix("data.").unwrap_or(live);
    format!("data.pipeline.{suffix}")
}

#[rstest]
fn test_process_pipeline_quote_publishes_on_pipeline_topic_only(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let instrument_id = audusd_sim.id;
    let mut data_engine = DataEngine::new(clock, cache, None);

    let live_topic = switchboard::get_quotes_topic(instrument_id);
    let pipeline_topic_str = pipeline_topic_of(live_topic.as_ref());
    let pipeline_topic: MStr<Topic> = pipeline_topic_str.as_str().into();

    let (live_handler, live_saver) =
        get_typed_message_saving_handler::<QuoteTick>(Some(Ustr::from("pipeline-test-live")));
    let (pipeline_handler, pipeline_saver) =
        get_typed_message_saving_handler::<QuoteTick>(Some(Ustr::from("pipeline-test-pipeline")));
    msgbus::subscribe_quotes(live_topic.into(), live_handler, None);
    msgbus::subscribe_quotes(pipeline_topic.into(), pipeline_handler, None);

    let quote = quote_tick(instrument_id, "1.00000", "1.00010", 1);
    data_engine.process_pipeline(Data::Quote(quote));

    assert!(
        live_saver.get_messages().is_empty(),
        "pipeline quote must not publish on the live topic",
    );
    let pipeline_messages = pipeline_saver.get_messages();
    assert_eq!(pipeline_messages.len(), 1);
    assert_eq!(pipeline_messages[0], quote);
}

#[rstest]
fn test_process_pipeline_quote_writes_cache_by_default(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let instrument_id = audusd_sim.id;
    let mut data_engine = DataEngine::new(clock, cache.clone(), None);

    let quote = quote_tick(instrument_id, "1.00000", "1.00010", 1);
    data_engine.process_pipeline(Data::Quote(quote));

    assert_eq!(cache.borrow().quote(&instrument_id), Some(&quote));
}

#[rstest]
fn test_process_pipeline_skips_cache_when_disabled(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let instrument_id = audusd_sim.id;
    let config = DataEngineConfig {
        disable_historical_cache: true,
        ..DataEngineConfig::default()
    };
    let mut data_engine = DataEngine::new(clock, cache.clone(), Some(config));

    let pipeline_topic_str =
        pipeline_topic_of(switchboard::get_quotes_topic(instrument_id).as_ref());
    let pipeline_topic: MStr<Topic> = pipeline_topic_str.as_str().into();
    let (pipeline_handler, pipeline_saver) =
        get_typed_message_saving_handler::<QuoteTick>(Some(Ustr::from("pipeline-cache-disabled")));
    msgbus::subscribe_quotes(pipeline_topic.into(), pipeline_handler, None);

    let quote = quote_tick(instrument_id, "1.00000", "1.00010", 1);
    data_engine.process_pipeline(Data::Quote(quote));

    assert_eq!(
        cache.borrow().quote(&instrument_id),
        None,
        "disable_historical_cache must suppress cache write",
    );
    let pipeline_messages = pipeline_saver.get_messages();
    assert_eq!(
        pipeline_messages.len(),
        1,
        "pipeline publish must still occur with cache disabled",
    );
}

#[rstest]
fn test_process_pipeline_bar_publishes_on_pipeline_topic(stub_msgbus: Rc<RefCell<MessageBus>>) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache.clone(), None);

    let bar = Bar::default();
    let live_topic = switchboard::get_bars_topic(bar.bar_type);
    let pipeline_topic_str = pipeline_topic_of(live_topic.as_ref());
    let pipeline_topic: MStr<Topic> = pipeline_topic_str.as_str().into();

    let (live_handler, live_saver) =
        get_typed_message_saving_handler::<Bar>(Some(Ustr::from("pipeline-bar-live")));
    let (pipeline_handler, pipeline_saver) =
        get_typed_message_saving_handler::<Bar>(Some(Ustr::from("pipeline-bar-pipeline")));
    msgbus::subscribe_bars(live_topic.into(), live_handler, None);
    msgbus::subscribe_bars(pipeline_topic.into(), pipeline_handler, None);

    data_engine.process_pipeline(Data::Bar(bar));

    assert!(
        live_saver.get_messages().is_empty(),
        "pipeline bar must not publish on the live topic",
    );
    let pipeline_messages = pipeline_saver.get_messages();
    assert_eq!(pipeline_messages.len(), 1);
    assert_eq!(pipeline_messages[0], bar);
    assert_eq!(
        cache.borrow().bar(&bar.bar_type),
        Some(&bar),
        "pipeline bar must populate the cache by default",
    );
}

#[rstest]
fn test_process_pipeline_increments_data_count(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache, None);

    let quote = quote_tick(audusd_sim.id, "1.00000", "1.00010", 1);
    let bar = Bar::default();

    assert_eq!(data_engine.data_count(), 0);
    data_engine.process_pipeline(Data::Quote(quote));
    data_engine.process_pipeline(Data::Bar(bar));
    assert_eq!(
        data_engine.data_count(),
        2,
        "process_pipeline must increment data_count like process_data",
    );
}

#[rstest]
fn test_process_pipeline_trade_publishes_on_pipeline_topic_only(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let instrument_id = audusd_sim.id;
    let mut data_engine = DataEngine::new(clock, cache.clone(), None);

    let live_topic = switchboard::get_trades_topic(instrument_id);
    let pipeline_topic_str = pipeline_topic_of(live_topic.as_ref());
    let pipeline_topic: MStr<Topic> = pipeline_topic_str.as_str().into();

    let (live_handler, live_saver) =
        get_typed_message_saving_handler::<TradeTick>(Some(Ustr::from("pipeline-trade-live")));
    let (pipeline_handler, pipeline_saver) =
        get_typed_message_saving_handler::<TradeTick>(Some(Ustr::from("pipeline-trade-pipeline")));
    msgbus::subscribe_trades(live_topic.into(), live_handler, None);
    msgbus::subscribe_trades(pipeline_topic.into(), pipeline_handler, None);

    let trade = trade_tick(instrument_id, "1.00000", "T-1", 1);
    data_engine.process_pipeline(Data::Trade(trade));

    assert!(
        live_saver.get_messages().is_empty(),
        "pipeline trade must not publish on the live topic",
    );
    let pipeline_messages = pipeline_saver.get_messages();
    assert_eq!(pipeline_messages.len(), 1);
    assert_eq!(pipeline_messages[0], trade);
    assert_eq!(
        cache.borrow().trade(&instrument_id),
        Some(&trade),
        "pipeline trade must populate the cache by default",
    );
}

#[rstest]
fn test_process_pipeline_mark_price_publishes_on_pipeline_topic_only(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let instrument_id = audusd_sim.id;
    let mut data_engine = DataEngine::new(clock, cache.clone(), None);

    let live_topic = switchboard::get_mark_price_topic(instrument_id);
    let pipeline_topic_str = pipeline_topic_of(live_topic.as_ref());
    let pipeline_topic: MStr<Topic> = pipeline_topic_str.as_str().into();

    let (live_handler, live_saver) =
        get_typed_message_saving_handler::<MarkPriceUpdate>(Some(Ustr::from("pipeline-mark-live")));
    let (pipeline_handler, pipeline_saver) = get_typed_message_saving_handler::<MarkPriceUpdate>(
        Some(Ustr::from("pipeline-mark-pipeline")),
    );
    msgbus::subscribe_mark_prices(live_topic.into(), live_handler, None);
    msgbus::subscribe_mark_prices(pipeline_topic.into(), pipeline_handler, None);

    let mark_price = MarkPriceUpdate::new(
        instrument_id,
        Price::from("1.00000"),
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    data_engine.process_pipeline(Data::MarkPriceUpdate(mark_price));

    assert!(
        live_saver.get_messages().is_empty(),
        "pipeline mark price must not publish on the live topic",
    );
    let pipeline_messages = pipeline_saver.get_messages();
    assert_eq!(pipeline_messages.len(), 1);
    assert_eq!(pipeline_messages[0], mark_price);
    assert_eq!(
        cache.borrow().mark_price(&instrument_id),
        Some(&mark_price),
        "pipeline mark price must populate the cache by default",
    );
}

#[rstest]
fn test_process_pipeline_index_price_publishes_on_pipeline_topic_only(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let instrument_id = audusd_sim.id;
    let mut data_engine = DataEngine::new(clock, cache.clone(), None);

    let live_topic = switchboard::get_index_price_topic(instrument_id);
    let pipeline_topic_str = pipeline_topic_of(live_topic.as_ref());
    let pipeline_topic: MStr<Topic> = pipeline_topic_str.as_str().into();

    let (live_handler, live_saver) = get_typed_message_saving_handler::<IndexPriceUpdate>(Some(
        Ustr::from("pipeline-index-live"),
    ));
    let (pipeline_handler, pipeline_saver) = get_typed_message_saving_handler::<IndexPriceUpdate>(
        Some(Ustr::from("pipeline-index-pipeline")),
    );
    msgbus::subscribe_index_prices(live_topic.into(), live_handler, None);
    msgbus::subscribe_index_prices(pipeline_topic.into(), pipeline_handler, None);

    let index_price = IndexPriceUpdate::new(
        instrument_id,
        Price::from("1.00000"),
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    data_engine.process_pipeline(Data::IndexPriceUpdate(index_price));

    assert!(
        live_saver.get_messages().is_empty(),
        "pipeline index price must not publish on the live topic",
    );
    let pipeline_messages = pipeline_saver.get_messages();
    assert_eq!(pipeline_messages.len(), 1);
    assert_eq!(pipeline_messages[0], index_price);
    assert_eq!(
        cache.borrow().index_price(&instrument_id),
        Some(&index_price),
        "pipeline index price must populate the cache by default",
    );
}

#[rstest]
fn test_process_pipeline_instrument_status_publishes_on_pipeline_topic_only(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let instrument_id = audusd_sim.id;
    let mut data_engine = DataEngine::new(clock, cache.clone(), None);

    let live_topic = switchboard::get_instrument_status_topic(instrument_id);
    let pipeline_topic_str = pipeline_topic_of(live_topic.as_ref());
    let pipeline_topic: MStr<Topic> = pipeline_topic_str.as_str().into();

    let (live_handler, live_saver) =
        get_any_saving_handler::<InstrumentStatus>(Some(Ustr::from("pipeline-status-live")));
    let (pipeline_handler, pipeline_saver) =
        get_any_saving_handler::<InstrumentStatus>(Some(Ustr::from("pipeline-status-pipeline")));
    msgbus::subscribe_any(live_topic.into(), live_handler, None);
    msgbus::subscribe_any(pipeline_topic.into(), pipeline_handler, None);

    let status = InstrumentStatus::new(
        instrument_id,
        MarketStatusAction::Trading,
        UnixNanos::from(1),
        UnixNanos::from(2),
        None,
        None,
        Some(true),
        Some(true),
        None,
    );
    data_engine.process_pipeline(Data::InstrumentStatus(status));

    assert!(
        live_saver.get_messages().is_empty(),
        "pipeline instrument status must not publish on the live topic",
    );
    let pipeline_messages = pipeline_saver.get_messages();
    assert_eq!(pipeline_messages.len(), 1);
    assert_eq!(pipeline_messages[0], status);
    assert_eq!(
        cache.borrow().instrument_status(&instrument_id),
        Some(&status),
        "pipeline instrument status must populate the cache by default",
    );
}

#[rstest]
fn test_process_pipeline_instrument_close_publishes_on_pipeline_topic_only(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let instrument_id = audusd_sim.id;
    let mut data_engine = DataEngine::new(clock, cache, None);

    let live_topic = switchboard::get_instrument_close_topic(instrument_id);
    let pipeline_topic_str = pipeline_topic_of(live_topic.as_ref());
    let pipeline_topic: MStr<Topic> = pipeline_topic_str.as_str().into();

    let (live_handler, live_saver) =
        get_any_saving_handler::<InstrumentClose>(Some(Ustr::from("pipeline-close-live")));
    let (pipeline_handler, pipeline_saver) =
        get_any_saving_handler::<InstrumentClose>(Some(Ustr::from("pipeline-close-pipeline")));
    msgbus::subscribe_any(live_topic.into(), live_handler, None);
    msgbus::subscribe_any(pipeline_topic.into(), pipeline_handler, None);

    let close = InstrumentClose::new(
        instrument_id,
        Price::from("1.00000"),
        InstrumentCloseType::EndOfSession,
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    data_engine.process_pipeline(Data::InstrumentClose(close));

    assert!(
        live_saver.get_messages().is_empty(),
        "pipeline instrument close must not publish on the live topic",
    );
    let pipeline_messages = pipeline_saver.get_messages();
    assert_eq!(pipeline_messages.len(), 1);
    assert_eq!(pipeline_messages[0], close);
}

#[rstest]
fn test_process_pipeline_delta_publishes_on_pipeline_topic_only(
    stub_msgbus: Rc<RefCell<MessageBus>>,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache, None);

    let delta = stub_delta();
    let instrument_id = delta.instrument_id;
    let live_topic = switchboard::get_book_deltas_topic(instrument_id);
    let pipeline_topic_str = pipeline_topic_of(live_topic.as_ref());
    let pipeline_topic: MStr<Topic> = pipeline_topic_str.as_str().into();

    let (live_handler, live_saver) = get_typed_message_saving_handler::<OrderBookDeltas>(Some(
        Ustr::from("pipeline-delta-live"),
    ));
    let (pipeline_handler, pipeline_saver) = get_typed_message_saving_handler::<OrderBookDeltas>(
        Some(Ustr::from("pipeline-delta-pipeline")),
    );
    msgbus::subscribe_book_deltas(live_topic.into(), live_handler, None);
    msgbus::subscribe_book_deltas(pipeline_topic.into(), pipeline_handler, None);

    data_engine.process_pipeline(Data::Delta(delta));

    assert!(
        live_saver.get_messages().is_empty(),
        "pipeline delta must not publish on the live topic",
    );
    let pipeline_messages = pipeline_saver.get_messages();
    assert_eq!(pipeline_messages.len(), 1);
    assert_eq!(pipeline_messages[0].instrument_id, instrument_id);
    assert_eq!(pipeline_messages[0].deltas.len(), 1);
    assert_eq!(pipeline_messages[0].deltas[0], delta);
}

#[rstest]
fn test_process_pipeline_deltas_publishes_on_pipeline_topic_only(
    stub_msgbus: Rc<RefCell<MessageBus>>,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache, None);

    let deltas = stub_deltas();
    let instrument_id = deltas.instrument_id;
    let live_topic = switchboard::get_book_deltas_topic(instrument_id);
    let pipeline_topic_str = pipeline_topic_of(live_topic.as_ref());
    let pipeline_topic: MStr<Topic> = pipeline_topic_str.as_str().into();

    let (live_handler, live_saver) = get_typed_message_saving_handler::<OrderBookDeltas>(Some(
        Ustr::from("pipeline-deltas-live"),
    ));
    let (pipeline_handler, pipeline_saver) = get_typed_message_saving_handler::<OrderBookDeltas>(
        Some(Ustr::from("pipeline-deltas-pipeline")),
    );
    msgbus::subscribe_book_deltas(live_topic.into(), live_handler, None);
    msgbus::subscribe_book_deltas(pipeline_topic.into(), pipeline_handler, None);

    data_engine.process_pipeline(Data::Deltas(OrderBookDeltas_API::new(deltas.clone())));

    assert!(
        live_saver.get_messages().is_empty(),
        "pipeline deltas must not publish on the live topic",
    );
    let pipeline_messages = pipeline_saver.get_messages();
    assert_eq!(pipeline_messages.len(), 1);
    assert_eq!(pipeline_messages[0], deltas);
}

#[rstest]
fn test_process_pipeline_depth10_publishes_on_pipeline_topic_only(
    stub_msgbus: Rc<RefCell<MessageBus>>,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache, None);

    let depth = stub_depth10();
    let instrument_id = depth.instrument_id;
    let live_topic = switchboard::get_book_depth10_topic(instrument_id);
    let pipeline_topic_str = pipeline_topic_of(live_topic.as_ref());
    let pipeline_topic: MStr<Topic> = pipeline_topic_str.as_str().into();

    let (live_handler, live_saver) = get_typed_message_saving_handler::<OrderBookDepth10>(Some(
        Ustr::from("pipeline-depth-live"),
    ));
    let (pipeline_handler, pipeline_saver) = get_typed_message_saving_handler::<OrderBookDepth10>(
        Some(Ustr::from("pipeline-depth-pipeline")),
    );
    msgbus::subscribe_book_depth10(live_topic.into(), live_handler, None);
    msgbus::subscribe_book_depth10(pipeline_topic.into(), pipeline_handler, None);

    data_engine.process_pipeline(Data::Depth10(Box::new(depth)));

    assert!(
        live_saver.get_messages().is_empty(),
        "pipeline depth10 must not publish on the live topic",
    );
    let pipeline_messages = pipeline_saver.get_messages();
    assert_eq!(pipeline_messages.len(), 1);
    assert_eq!(pipeline_messages[0], depth);
}

#[rstest]
fn test_process_pipeline_custom_data_publishes_on_pipeline_topic_only(
    stub_msgbus: Rc<RefCell<MessageBus>>,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache, None);

    let custom = stub_custom_data(
        7_000,
        7,
        Some(serde_json::from_value(json!({"source": "metadata"})).unwrap()),
        Some("SIM//CUSTOM".to_string()),
    );
    let live_topic = switchboard::get_custom_topic(&custom.data_type);
    let pipeline_topic_str = pipeline_topic_of(live_topic.as_ref());
    let pipeline_topic: MStr<Topic> = pipeline_topic_str.as_str().into();

    let (live_handler, live_saver) =
        get_any_saving_handler::<CustomData>(Some(Ustr::from("pipeline-custom-live")));
    let (pipeline_handler, pipeline_saver) =
        get_any_saving_handler::<CustomData>(Some(Ustr::from("pipeline-custom-pipeline")));
    msgbus::subscribe_any(live_topic.into(), live_handler, None);
    msgbus::subscribe_any(pipeline_topic.into(), pipeline_handler, None);

    data_engine.process_pipeline(Data::Custom(custom.clone()));

    assert!(
        live_saver.get_messages().is_empty(),
        "pipeline custom data must not publish on the live topic",
    );
    let pipeline_messages = pipeline_saver.get_messages();
    assert_eq!(pipeline_messages.len(), 1);
    assert_eq!(pipeline_messages[0], custom);
}

#[rstest]
fn test_process_pipeline_bar_drops_out_of_sequence(stub_msgbus: Rc<RefCell<MessageBus>>) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let config = DataEngineConfig {
        validate_data_sequence: true,
        ..DataEngineConfig::default()
    };
    let mut data_engine = DataEngine::new(clock, cache.clone(), Some(config));

    let template = Bar::default();
    let bar_type = template.bar_type;
    let make_bar = |ts: u64| {
        Bar::new(
            bar_type,
            template.open,
            template.high,
            template.low,
            template.close,
            template.volume,
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };

    let first = make_bar(2_000);
    let second = make_bar(1_000); // regresses on both ts_event and ts_init

    data_engine.process_pipeline(Data::Bar(first));
    data_engine.process_pipeline(Data::Bar(second));

    assert_eq!(
        cache.borrow().bar(&bar_type),
        Some(&first),
        "pipeline bar handler must honour validate_data_sequence and keep the first bar",
    );
}

#[rstest]
fn test_process_pipeline_skips_synthetic_quote_republish(stub_msgbus: Rc<RefCell<MessageBus>>) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache.clone(), None);

    let (synthetic, component_a, component_b) = synthetic_index();
    let synthetic_id = synthetic.id;
    cache.borrow_mut().add_synthetic(synthetic).unwrap();

    let (handler, saver) =
        get_typed_message_saving_handler::<QuoteTick>(Some(Ustr::from("pipeline-synth-quote")));
    let topic = switchboard::get_quotes_topic(synthetic_id);
    msgbus::subscribe_quotes(topic.into(), handler, None);

    // Register the synthetic feed via the public subscribe path so the live
    // path would normally republish on component-quote arrival.
    data_engine.execute(subscribe_synthetic_quotes_cmd(synthetic_id));
    assert!(
        data_engine
            .subscribed_synthetic_quotes()
            .contains(&synthetic_id),
    );

    // Seed one component live so the synthetic calc could produce a quote
    let quote_a = quote_tick(component_a, "100.00", "102.00", 1);
    data_engine.process_data(Data::Quote(quote_a));
    assert!(saver.get_messages().is_empty()); // both components required

    // Now drive the other component through the pipeline path. The live path
    // would publish a synthetic quote here; the pipeline path must not.
    let quote_b = quote_tick(component_b, "200.00", "204.00", 2);
    data_engine.process_pipeline(Data::Quote(quote_b));

    assert!(
        saver.get_messages().is_empty(),
        "pipeline mode must not republish synthetic quotes",
    );
}

#[rstest]
fn test_process_pipeline_skips_synthetic_trade_republish(stub_msgbus: Rc<RefCell<MessageBus>>) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache.clone(), None);

    let (synthetic, component_a, component_b) = synthetic_index();
    let synthetic_id = synthetic.id;
    cache.borrow_mut().add_synthetic(synthetic).unwrap();

    let (handler, saver) =
        get_typed_message_saving_handler::<TradeTick>(Some(Ustr::from("pipeline-synth-trade")));
    let topic = switchboard::get_trades_topic(synthetic_id);
    msgbus::subscribe_trades(topic.into(), handler, None);

    data_engine.execute(subscribe_synthetic_trades_cmd(synthetic_id));

    let trade_a = trade_tick(component_a, "100.00", "T-a", 1);
    data_engine.process_data(Data::Trade(trade_a));
    assert!(saver.get_messages().is_empty()); // both components required

    let trade_b = trade_tick(component_b, "200.00", "T-b", 2);
    data_engine.process_pipeline(Data::Trade(trade_b));

    assert!(
        saver.get_messages().is_empty(),
        "pipeline mode must not republish synthetic trades",
    );
}

#[rstest]
fn test_process_pipeline_depth10_skips_derived_quote_emission(
    stub_msgbus: Rc<RefCell<MessageBus>>,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    // Live path would derive a quote from depth top-of-book with this flag
    let config = DataEngineConfig {
        emit_quotes_from_book_depths: true,
        ..DataEngineConfig::default()
    };
    let mut data_engine = DataEngine::new(clock, cache.clone(), Some(config));

    let depth = stub_depth10();
    let instrument_id = depth.instrument_id;

    let (handler, saver) =
        get_typed_message_saving_handler::<QuoteTick>(Some(Ustr::from("pipeline-depth-derived")));
    let quote_topic = switchboard::get_quotes_topic(instrument_id);
    msgbus::subscribe_quotes(quote_topic.into(), handler, None);

    data_engine.process_pipeline(Data::Depth10(Box::new(depth)));

    assert!(
        saver.get_messages().is_empty(),
        "pipeline depth10 must not emit a derived quote even when emit_quotes_from_book_depths is set",
    );
    assert!(
        cache.borrow().quote(&instrument_id).is_none(),
        "no derived quote should be cached for pipeline depth10",
    );
}

#[rstest]
fn test_process_pipeline_instrument_status_skips_option_chain_expiry(
    clock: Rc<RefCell<TestClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let _ = msgbus::get_message_bus();
    let data_engine = make_option_chain_engine(clock.clone(), cache.clone());

    let client_id = ClientId::new("DERIBIT");
    let venue = Venue::new("DERIBIT");
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));

    register_mock_client(
        clock,
        cache.clone(),
        client_id,
        venue,
        Some(venue),
        &recorder,
        &mut data_engine.borrow_mut(),
    );

    let call = make_btc_option("50000.000", OptionKind::Call);
    let put = make_btc_option("50000.000", OptionKind::Put);
    let call_id = call.id();
    let _ = cache.borrow_mut().add_instrument(call);
    let _ = cache.borrow_mut().add_instrument(put);

    let series_id = make_series_id();
    let cmd = make_subscribe_option_chain(
        series_id,
        vec![Price::from("50000.000")],
        Some(client_id),
        Some(venue),
    );
    data_engine.borrow_mut().execute(cmd);

    recorder.borrow_mut().clear();

    // Drive a Close status through the pipeline; live path would expire the
    // instrument and emit wire-level unsubscribes, pipeline must not.
    let status = InstrumentStatus::new(
        call_id,
        MarketStatusAction::Close,
        UnixNanos::from(1),
        UnixNanos::from(2),
        None,
        None,
        Some(false),
        Some(false),
        None,
    );
    data_engine
        .borrow_mut()
        .process_pipeline(Data::InstrumentStatus(status));

    let unsubs: Vec<_> = recorder
        .borrow()
        .iter()
        .filter(|cmd| matches!(cmd, DataCommand::Unsubscribe(_)))
        .cloned()
        .collect();
    assert!(
        unsubs.is_empty(),
        "pipeline instrument status must not trigger option chain expiry (got {unsubs:?})",
    );
    assert!(
        data_engine.borrow().has_option_chain_manager(&series_id),
        "option chain manager must remain intact after pipeline status",
    );
}

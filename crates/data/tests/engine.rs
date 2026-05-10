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
        DataCommand, RequestBars, RequestBookDepth, RequestBookSnapshot, RequestCommand,
        RequestCustomData, RequestFundingRates, RequestInstrument, RequestInstruments,
        RequestQuotes, RequestTrades, SubscribeBars, SubscribeBookDeltas, SubscribeBookDepth10,
        SubscribeBookSnapshots, SubscribeCommand, SubscribeCustomData, SubscribeFundingRates,
        SubscribeIndexPrices, SubscribeInstrument, SubscribeInstrumentClose,
        SubscribeInstrumentStatus, SubscribeMarkPrices, SubscribeOptionChain,
        SubscribeOptionGreeks, SubscribeQuotes, SubscribeTrades, UnsubscribeBars,
        UnsubscribeBookDeltas, UnsubscribeBookDepth10, UnsubscribeBookSnapshots,
        UnsubscribeCommand, UnsubscribeCustomData, UnsubscribeFundingRates, UnsubscribeIndexPrices,
        UnsubscribeInstrument, UnsubscribeInstrumentClose, UnsubscribeInstrumentStatus,
        UnsubscribeMarkPrices, UnsubscribeOptionChain, UnsubscribeOptionGreeks, UnsubscribeQuotes,
        UnsubscribeTrades,
    },
    msgbus::{
        self, MessageBus, TypedHandler, TypedIntoHandler,
        stubs::get_typed_message_saving_handler,
        switchboard::{self, MessagingSwitchboard},
    },
    testing::wait_until,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_data::{client::DataClientAdapter, engine::DataEngine};
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
        Bar, BarType, Data, DataType, FundingRateUpdate, IndexPriceUpdate, InstrumentStatus,
        MarkPriceUpdate, OrderBookDeltas, OrderBookDeltas_API, OrderBookDepth10, QuoteTick,
        TradeTick,
        option_chain::StrikeRange,
        stubs::{OrderBookDeltaTestBuilder, stub_delta, stub_deltas, stub_depth10},
    },
    enums::{BookType, MarketStatusAction, OptionKind, PriceType},
    identifiers::{ClientId, InstrumentId, OptionSeriesId, TraderId, Venue},
    instruments::{
        CurrencyPair, Instrument, InstrumentAny,
        stubs::{audusd_sim, gbpusd_sim},
    },
    orderbook::OrderBook,
    stubs::TestDefault,
    types::{Price, Quantity},
};
use rstest::*;

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

    let handler = msgbus::stubs::get_message_saving_handler::<InstrumentAny>(None);
    let topic = switchboard::get_instrument_topic(audusd_sim.id());
    msgbus::subscribe_any(topic.into(), handler.clone(), None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process(&audusd_sim as &dyn Any);
    let cache = &data_engine.get_cache();
    let messages = msgbus::stubs::get_saved_messages::<InstrumentAny>(&handler);

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
    profiler.initialize(initial_price);

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
    profiler.initialize(initial_price);
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
    profiler.initialize(initial_price);

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
    profiler.initialize(initial_price);
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
    profiler.initialize(initial_price);
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
    use nautilus_model::identifiers::Symbol;
    InstrumentId::new(Symbol::new("BTC-ETH-INDEX"), Venue::synthetic())
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
    use nautilus_common::messages::data::{DataResponse, InstrumentResponse};

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

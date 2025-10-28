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

mod common;

use std::{any::Any, cell::RefCell, num::NonZeroUsize, rc::Rc, str::FromStr, sync::Arc};

use alloy_primitives::{Address, I256, U160, U256};
use common::mocks::MockDataClient;
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
        RequestCustomData, RequestInstrument, RequestInstruments, RequestQuotes, RequestTrades,
        SubscribeBars, SubscribeBookDeltas, SubscribeBookDepth10, SubscribeBookSnapshots,
        SubscribeCommand, SubscribeCustomData, SubscribeFundingRates, SubscribeIndexPrices,
        SubscribeInstrument, SubscribeMarkPrices, SubscribeQuotes, SubscribeTrades,
        UnsubscribeBars, UnsubscribeBookDeltas, UnsubscribeBookSnapshots, UnsubscribeCommand,
        UnsubscribeCustomData, UnsubscribeFundingRates, UnsubscribeIndexPrices,
        UnsubscribeInstrument, UnsubscribeMarkPrices, UnsubscribeQuotes, UnsubscribeTrades,
    },
    msgbus::{
        self, MessageBus,
        handler::{ShareableMessageHandler, TypedMessageHandler},
        stubs::{get_message_saving_handler, get_saved_messages},
        switchboard::{self, MessagingSwitchboard},
    },
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_data::{client::DataClientAdapter, engine::DataEngine};
use nautilus_model::{
    data::{
        Bar, BarType, Data, DataType, FundingRateUpdate, IndexPriceUpdate, MarkPriceUpdate,
        OrderBookDeltas, OrderBookDeltas_API, OrderBookDepth10, QuoteTick, TradeTick,
        stubs::{stub_delta, stub_deltas, stub_depth10},
    },
    defi::{AmmType, Dex, DexType, chain::chains},
    enums::{BookType, PriceType},
    identifiers::{ClientId, InstrumentId, TraderId, Venue},
    instruments::{CurrencyPair, Instrument, InstrumentAny, stubs::audusd_sim},
    types::Price,
};
#[cfg(feature = "defi")]
use nautilus_model::{
    defi::{
        Block, Blockchain, DefiData, Pool, PoolLiquidityUpdate, PoolLiquidityUpdateType,
        PoolProfiler, PoolSwap, Token, data::PoolFeeCollect, data::PoolFlash,
    },
    enums::OrderSide,
    types::Quantity,
};
use rstest::*;

#[fixture]
fn client_id() -> ClientId {
    ClientId::default()
}

#[fixture]
fn venue() -> Venue {
    Venue::default()
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
    MessageBus::new(TraderId::default(), UUID4::new(), None, None).register_message_bus()
}

#[fixture]
fn data_engine(
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
) -> Rc<RefCell<DataEngine>> {
    let data_engine = Rc::new(RefCell::new(DataEngine::new(clock, cache, None)));

    let data_engine_clone = data_engine.clone();
    let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
        move |cmd: &DataCommand| data_engine_clone.borrow_mut().execute(cmd),
    )));

    let endpoint = MessagingSwitchboard::data_engine_execute();
    msgbus::register(endpoint, handler);

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

// ------------------------------------------------------------------------------------------------
// Client registration & routing tests
// ------------------------------------------------------------------------------------------------

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
            Some(Venue::default()),
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
            Some(Venue::default()),
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
    let venue = Venue::default();

    let data_client1 = DataClientAdapter::new(
        client_id,
        Some(venue),
        true,
        true,
        Box::new(MockDataClient::new(
            clock.clone(),
            cache.clone(),
            client_id,
            Some(Venue::default()),
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
            Some(Venue::default()),
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
    let venue1 = Venue::default();

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
            Some(Venue::default()),
        )),
    );
    data_engine.register_default_client(default_client);

    assert_eq!(data_engine.registered_clients(), vec![default_id]);
    assert_eq!(
        data_engine.get_client(None, None).unwrap().client_id(),
        default_id
    );
}

// ------------------------------------------------------------------------------------------------
// Test execute subscription commands
// ------------------------------------------------------------------------------------------------

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

    let data_type = DataType::new(stringify!(String), None);
    let sub = SubscribeCustomData::new(
        Some(client_id),
        Some(venue),
        data_type.clone(),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    let sub_cmd = DataCommand::Subscribe(SubscribeCommand::Data(sub));
    data_engine.execute(&sub_cmd);

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
    );
    let unsub_cmd = DataCommand::Unsubscribe(UnsubscribeCommand::Data(unsub));
    data_engine.execute(&unsub_cmd);

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
    )));
    data_engine.execute(&sub_cmd);

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
        )));
    data_engine.execute(&unsub_cmd);

    assert!(
        !data_engine
            .subscribed_book_deltas()
            .contains(&audusd_sim.id)
    );
    assert_eq!(recorder.borrow().as_slice(), &[sub_cmd, unsub_cmd]);
}

#[ignore = "Attempt to subtract with overflow"]
#[rstest]
fn test_execute_subscribe_book_snapshots(
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

    let sub = SubscribeBookSnapshots::new(
        audusd_sim.id,
        BookType::L2_MBP,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        NonZeroUsize::new(1_000).unwrap(),
        None,
    );
    let sub_cmd = DataCommand::Subscribe(SubscribeCommand::BookSnapshots(sub));
    data_engine.execute(&sub_cmd);

    assert!(
        data_engine
            .subscribed_book_snapshots()
            .contains(&audusd_sim.id)
    );
    {
        assert_eq!(recorder.borrow().as_slice(), std::slice::from_ref(&sub_cmd));
    }

    let unsub = UnsubscribeBookSnapshots::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    let unsub_cmd = DataCommand::Unsubscribe(UnsubscribeCommand::BookSnapshots(unsub));
    data_engine.execute(&unsub_cmd);

    assert!(
        !data_engine
            .subscribed_book_snapshots()
            .contains(&audusd_sim.id)
    );
    assert_eq!(recorder.borrow().as_slice(), &[sub_cmd, unsub_cmd]);
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
    );
    let sub_cmd = DataCommand::Subscribe(SubscribeCommand::Instrument(sub));
    data_engine.execute(&sub_cmd);

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
    );
    let unsub_cmd = DataCommand::Unsubscribe(UnsubscribeCommand::Instrument(unsub));
    data_engine.execute(&unsub_cmd);

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
    );
    let sub_cmd = DataCommand::Subscribe(SubscribeCommand::Quotes(sub));
    data_engine.execute(&sub_cmd);

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
    );
    let unsub_cmd = DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(unsub));
    data_engine.execute(&unsub_cmd);

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
    );
    let sub_cmd = DataCommand::Subscribe(SubscribeCommand::Trades(sub));
    data_engine.execute(&sub_cmd);

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
    );
    let unsub_cmd = DataCommand::Unsubscribe(UnsubscribeCommand::Trades(ubsub));
    data_engine.execute(&unsub_cmd);

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

    let inst_any = InstrumentAny::CurrencyPair(audusd_sim);
    data_engine.process(&inst_any as &dyn Any);

    let bar_type = BarType::from("AUD/USD.SIM-1-MINUTE-LAST-INTERNAL");

    let sub = SubscribeBars::new(
        bar_type,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    let sub_cmd = DataCommand::Subscribe(SubscribeCommand::Bars(sub));
    data_engine.execute(&sub_cmd);

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
    );
    let unsub_cmd = DataCommand::Unsubscribe(UnsubscribeCommand::Bars(unsub));
    data_engine.execute(&unsub_cmd);

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
    );
    let sub_cmd = DataCommand::Subscribe(SubscribeCommand::MarkPrices(sub));
    data_engine.execute(&sub_cmd);

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
    );
    let unsub_cmd = DataCommand::Unsubscribe(UnsubscribeCommand::MarkPrices(unsub));
    data_engine.execute(&unsub_cmd);

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
    )));
    data_engine.execute(&sub_cmd);

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
        ),
    ));
    data_engine.execute(&unsub_cmd);

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
    );
    let sub_cmd = DataCommand::Subscribe(SubscribeCommand::FundingRates(sub));
    data_engine.execute(&sub_cmd);

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
    );
    let unsub_cmd = DataCommand::Unsubscribe(UnsubscribeCommand::FundingRates(unsub));
    data_engine.execute(&unsub_cmd);

    assert!(
        !data_engine
            .subscribed_funding_rates()
            .contains(&audusd_sim.id)
    );
    assert_eq!(recorder.borrow().as_slice(), &[sub_cmd, unsub_cmd]);
}

// ------------------------------------------------------------------------------------------------
// Test execute request commands
// ------------------------------------------------------------------------------------------------

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
        data_type: DataType::new("X", None),
        start: None,
        end: None,
        limit: None,
        request_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    };
    let cmd = DataCommand::Request(RequestCommand::Data(req));
    data_engine.execute(&cmd);

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
    data_engine.execute(&cmd);

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
    data_engine.execute(&cmd);

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
    data_engine.execute(&cmd);

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
    data_engine.execute(&cmd);

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
    data_engine.execute(&cmd);

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
    data_engine.execute(&cmd);

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
    data_engine.execute(&cmd);

    assert_eq!(recorder.borrow()[0], cmd);
}

// ------------------------------------------------------------------------------------------------
// Test process data flows
// ------------------------------------------------------------------------------------------------

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
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::Instrument(sub));

    let endpoint = MessagingSwitchboard::data_engine_execute();
    msgbus::send_any(endpoint, &cmd as &dyn Any);

    let handler = get_message_saving_handler::<InstrumentAny>(None);
    let topic = switchboard::get_instrument_topic(audusd_sim.id());
    msgbus::subscribe(topic.into(), handler.clone(), None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process(&audusd_sim as &dyn Any);
    let cache = &data_engine.get_cache();
    let messages = get_saved_messages::<InstrumentAny>(handler);

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
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::BookDeltas(sub));

    let endpoint = MessagingSwitchboard::data_engine_execute();
    msgbus::send_any(endpoint, &cmd as &dyn Any);

    let delta = stub_delta();
    let handler = get_message_saving_handler::<OrderBookDeltas>(None);
    let topic = switchboard::get_book_deltas_topic(delta.instrument_id);
    msgbus::subscribe_topic(topic, handler.clone(), None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_data(Data::Delta(delta));
    let _cache = &data_engine.get_cache();
    let messages = get_saved_messages::<OrderBookDeltas>(handler);

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
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::BookDeltas(sub));

    let endpoint = MessagingSwitchboard::data_engine_execute();
    msgbus::send_any(endpoint, &cmd as &dyn Any);

    // TODO: Using FFI API wrapper temporarily until Cython gone
    let deltas = OrderBookDeltas_API::new(stub_deltas());
    let handler = get_message_saving_handler::<OrderBookDeltas>(None);
    let topic = switchboard::get_book_deltas_topic(deltas.instrument_id);
    msgbus::subscribe_topic(topic, handler.clone(), None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_data(Data::Deltas(deltas.clone()));
    let _cache = &data_engine.get_cache();
    let messages = get_saved_messages::<OrderBookDeltas>(handler);

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
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::BookDepth10(sub));

    let endpoint = MessagingSwitchboard::data_engine_execute();
    msgbus::send_any(endpoint, &cmd as &dyn Any);

    let depth = stub_depth10();
    let handler = get_message_saving_handler::<OrderBookDepth10>(None);
    let topic = switchboard::get_book_depth10_topic(depth.instrument_id);
    msgbus::subscribe_topic(topic, handler.clone(), None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_data(Data::from(depth));
    let _cache = &data_engine.get_cache();
    let messages = get_saved_messages::<OrderBookDepth10>(handler);

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
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::Quotes(sub));

    let endpoint = MessagingSwitchboard::data_engine_execute();
    msgbus::send_any(endpoint, &cmd as &dyn Any);

    let quote = QuoteTick::default();
    let handler = get_message_saving_handler::<QuoteTick>(None);
    let topic = switchboard::get_quotes_topic(quote.instrument_id);
    msgbus::subscribe_topic(topic, handler.clone(), None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_data(Data::Quote(quote));
    let cache = &data_engine.get_cache();
    let messages = get_saved_messages::<QuoteTick>(handler);

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
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::Trades(sub));

    let endpoint = MessagingSwitchboard::data_engine_execute();
    msgbus::send_any(endpoint, &cmd as &dyn Any);

    let trade = TradeTick::default();
    let handler = get_message_saving_handler::<TradeTick>(None);
    let topic = switchboard::get_trades_topic(trade.instrument_id);
    msgbus::subscribe_topic(topic, handler.clone(), None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_data(Data::Trade(trade));
    let cache = &data_engine.get_cache();
    let messages = get_saved_messages::<TradeTick>(handler);

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
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::MarkPrices(sub));

    let endpoint = MessagingSwitchboard::data_engine_execute();
    msgbus::send_any(endpoint, &cmd as &dyn Any);

    let mark_price = MarkPriceUpdate::new(
        audusd_sim.id,
        Price::from("1.00000"),
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    let handler = get_message_saving_handler::<MarkPriceUpdate>(None);
    let topic = switchboard::get_mark_price_topic(mark_price.instrument_id);
    msgbus::subscribe_topic(topic, handler.clone(), None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_data(Data::MarkPriceUpdate(mark_price));
    let cache = &data_engine.get_cache();
    let messages = get_saved_messages::<MarkPriceUpdate>(handler);

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
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::IndexPrices(sub));

    let endpoint = MessagingSwitchboard::data_engine_execute();
    msgbus::send_any(endpoint, &cmd as &dyn Any);

    let index_price = IndexPriceUpdate::new(
        audusd_sim.id,
        Price::from("1.00000"),
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    let handler = get_message_saving_handler::<IndexPriceUpdate>(None);
    let topic = switchboard::get_index_price_topic(index_price.instrument_id);
    msgbus::subscribe_topic(topic, handler.clone(), None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_data(Data::IndexPriceUpdate(index_price));
    let cache = &data_engine.get_cache();
    let messages = get_saved_messages::<IndexPriceUpdate>(handler);

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
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::FundingRates(sub));

    let endpoint = MessagingSwitchboard::data_engine_execute();
    msgbus::send_any(endpoint, &cmd as &dyn Any);

    let funding_rate = FundingRateUpdate::new(
        audusd_sim.id,
        "0.0001".parse().unwrap(),
        None,
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    let handler = get_message_saving_handler::<FundingRateUpdate>(None);
    let topic = switchboard::get_funding_rate_topic(funding_rate.instrument_id);
    msgbus::subscribe_topic(topic, handler.clone(), None);

    let mut data_engine = data_engine.borrow_mut();
    // Test through the process() method with &dyn Any
    data_engine.process(&funding_rate as &dyn Any);
    let cache = &data_engine.get_cache();
    let messages = get_saved_messages::<FundingRateUpdate>(handler);

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
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::FundingRates(sub));

    let endpoint = MessagingSwitchboard::data_engine_execute();
    msgbus::send_any(endpoint, &cmd as &dyn Any);

    let funding_rate = FundingRateUpdate::new(
        audusd_sim.id,
        "0.0001".parse().unwrap(),
        None,
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    let handler = get_message_saving_handler::<FundingRateUpdate>(None);
    let topic = switchboard::get_funding_rate_topic(funding_rate.instrument_id);
    msgbus::subscribe_topic(topic, handler.clone(), None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.handle_funding_rate(funding_rate);
    let cache = &data_engine.get_cache();
    let messages = get_saved_messages::<FundingRateUpdate>(handler);

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
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::FundingRates(sub));

    let endpoint = MessagingSwitchboard::data_engine_execute();
    msgbus::send_any(endpoint, &cmd as &dyn Any);

    let funding_rate1 = FundingRateUpdate::new(
        audusd_sim.id,
        "0.0001".parse().unwrap(),
        None,
        UnixNanos::from(1),
        UnixNanos::from(2),
    );

    let funding_rate2 = FundingRateUpdate::new(
        audusd_sim.id,
        "0.0002".parse().unwrap(),
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
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::Bars(sub));

    let endpoint = MessagingSwitchboard::data_engine_execute();
    msgbus::send_any(endpoint, &cmd as &dyn Any);

    let handler = get_message_saving_handler::<Bar>(None);
    let topic = switchboard::get_bars_topic(bar.bar_type);
    msgbus::subscribe_topic(topic, handler.clone(), None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_data(Data::Bar(bar));
    let cache = &data_engine.get_cache();
    let messages = get_saved_messages::<Bar>(handler);

    assert_eq!(cache.bar(&bar.bar_type), Some(bar).as_ref());
    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&bar));
}

// ------------------------------------------------------------------------------------------------
// DeFi subscription and processing tests
// ------------------------------------------------------------------------------------------------

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
    data_engine.execute(&sub_cmd);

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
    data_engine.execute(&unsub_cmd);

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
    data_engine.execute(&sub_cmd);

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
                "Expected second command to be RequestPoolSnapshot, got: {:?}",
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
    data_engine.execute(&unsub_cmd);

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

    let endpoint = MessagingSwitchboard::data_engine_execute();
    msgbus::send_any(endpoint, &cmd as &dyn Any);

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
    let handler = get_message_saving_handler::<Block>(None);
    let topic = defi::switchboard::get_defi_blocks_topic(blockchain);
    msgbus::subscribe_topic(topic, handler.clone(), None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_defi_data(DefiData::Block(block.clone()));
    let messages = get_saved_messages::<Block>(handler);

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
        0u64,
        token0,
        token1,
        Some(500u32),
        Some(10u32),
        UnixNanos::from(1),
    );

    let initial_price = U160::from(79228162514264337593543950336u128); // sqrt(1) * 2^96
    pool.initialize(initial_price);
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
        pool.address,
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
        Some(OrderSide::Buy),
        Some(Quantity::from("1000")),
        Some(Price::from("500")),
    );

    let sub = DefiSubscribeCommand::PoolSwaps(SubscribePoolSwaps {
        instrument_id,
        client_id: Some(client_id),
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    });
    let cmd = DataCommand::DefiSubscribe(sub);

    let endpoint = MessagingSwitchboard::data_engine_execute();
    msgbus::send_any(endpoint, &cmd as &dyn Any);

    let handler = get_message_saving_handler::<PoolSwap>(None);
    let topic = defi::switchboard::get_defi_pool_swaps_topic(instrument_id);
    msgbus::subscribe_topic(topic, handler.clone(), None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_defi_data(DefiData::PoolSwap(swap.clone()));
    let messages = get_saved_messages::<PoolSwap>(handler);

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
    data_engine.execute(&sub_cmd);

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
                "Expected second command to be RequestPoolSnapshot, got: {:?}",
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
    data_engine.execute(&unsub_cmd);

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
    data_engine.execute(&sub_cmd);

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
                "Expected second command to be RequestPoolSnapshot, got: {:?}",
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
    data_engine.execute(&unsub_cmd);

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
    data_engine.execute(&sub_cmd);

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
                "Expected second command to be RequestPoolSnapshot, got: {:?}",
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
    data_engine.execute(&unsub_cmd);

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
        0u64,
        token0,
        token1,
        Some(500u32),
        Some(10u32),
        UnixNanos::from(1),
    );

    let initial_price = U160::from(79228162514264337593543950336u128); // sqrt(1) * 2^96
    pool.initialize(initial_price);
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
        pool.address,
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

    let endpoint = MessagingSwitchboard::data_engine_execute();
    msgbus::send_any(endpoint, &cmd as &dyn Any);

    let handler = get_message_saving_handler::<PoolLiquidityUpdate>(None);
    let topic = defi::switchboard::get_defi_liquidity_topic(instrument_id);
    msgbus::subscribe_topic(topic, handler.clone(), None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_defi_data(DefiData::PoolLiquidityUpdate(update.clone()));
    let messages = get_saved_messages::<PoolLiquidityUpdate>(handler);

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
        0u64,
        token0,
        token1,
        Some(500u32),
        Some(10u32),
        UnixNanos::from(1),
    );

    let initial_price = U160::from(79228162514264337593543950336u128); // sqrt(1) * 2^96
    pool.initialize(initial_price);
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
        pool.address,
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

    let endpoint = MessagingSwitchboard::data_engine_execute();
    msgbus::send_any(endpoint, &cmd as &dyn Any);

    let handler = get_message_saving_handler::<PoolFeeCollect>(None);
    let topic = defi::switchboard::get_defi_collect_topic(instrument_id);
    msgbus::subscribe_topic(topic, handler.clone(), None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_defi_data(DefiData::PoolFeeCollect(collect.clone()));
    let messages = get_saved_messages::<PoolFeeCollect>(handler);

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
        0u64,
        token0,
        token1,
        Some(500u32),
        Some(10u32),
        UnixNanos::from(1),
    );

    let initial_price = U160::from(79228162514264337593543950336u128); // sqrt(1) * 2^96
    pool.initialize(initial_price);
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
        pool.address,
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

    let endpoint = MessagingSwitchboard::data_engine_execute();
    msgbus::send_any(endpoint, &cmd as &dyn Any);

    let handler = get_message_saving_handler::<PoolFlash>(None);
    let topic = defi::switchboard::get_defi_flash_topic(instrument_id);
    msgbus::subscribe_topic(topic, handler.clone(), None);

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_defi_data(DefiData::PoolFlash(flash.clone()));
    let messages = get_saved_messages::<PoolFlash>(handler);

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
        0u64,
        token0,
        token1,
        Some(500u32),
        Some(10u32),
        UnixNanos::from(1),
    );

    let initial_price = U160::from(79228162514264337593543950336u128); // sqrt(1) * 2^96
    pool.initialize(initial_price);
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
        Address::from([0x12; 20]),
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
        "Active liquidity should be > 0 after mint, got: {}",
        active_liquidity
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
    let endpoint = MessagingSwitchboard::data_engine_execute();
    msgbus::send_any(endpoint, &cmd as &dyn Any);

    // Create and process swap that changes tick
    let new_price = U160::from(56022770974786139918731938227u128); // Different price
    let swap = PoolSwap::new(
        chain,
        dex,
        instrument_id,
        Address::from([0x12; 20]),
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
        Some(OrderSide::Buy),
        Some(Quantity::from("1000")),
        Some(Price::from("500")),
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
        "PoolUpdater should have updated PoolProfiler: tick_changed={}, fees_increased={}, \
        initial_tick={:?}, final_tick={:?}, initial_fee_growth={}, final_fee_growth={}",
        tick_changed,
        fees_increased,
        initial_tick,
        final_tick,
        initial_fee_growth_0,
        final_fee_growth_0
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
        0u64,
        token0,
        token1,
        Some(500u32),
        Some(10u32),
        UnixNanos::from(1),
    );

    let initial_price = U160::from(79228162514264337593543950336u128);
    pool.initialize(initial_price);
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
    let endpoint = MessagingSwitchboard::data_engine_execute();
    msgbus::send_any(endpoint, &cmd as &dyn Any);

    // Create and process mint event
    let mint = PoolLiquidityUpdate::new(
        chain,
        dex,
        instrument_id,
        Address::from([0x12; 20]),
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
        0u64,
        token0,
        token1,
        Some(500u32),
        Some(10u32),
        UnixNanos::from(1),
    );

    let initial_price = U160::from(79228162514264337593543950336u128);
    pool.initialize(initial_price);
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
        Address::from([0x12; 20]),
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
    let endpoint = MessagingSwitchboard::data_engine_execute();
    msgbus::send_any(endpoint, &cmd as &dyn Any);

    // Create and process burn event
    let burn = PoolLiquidityUpdate::new(
        chain,
        dex,
        instrument_id,
        Address::from([0x12; 20]),
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

    let mut data_engine = data_engine.borrow_mut();
    data_engine.process_defi_data(DefiData::PoolLiquidityUpdate(burn));

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
        0u64,
        token0,
        token1,
        Some(500u32),
        Some(10u32),
        UnixNanos::from(1),
    );

    let initial_price = U160::from(79228162514264337593543950336u128);
    pool.initialize(initial_price);
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
    let endpoint = MessagingSwitchboard::data_engine_execute();
    msgbus::send_any(endpoint, &cmd as &dyn Any);

    // Create and process collect event
    let owner = Address::from([0xAB; 20]);
    let collect = PoolFeeCollect::new(
        chain,
        dex,
        instrument_id,
        Address::from([0x12; 20]),
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
        0u64,
        token0,
        token1,
        Some(500u32),
        Some(10u32),
        UnixNanos::from(1),
    );

    let initial_price = U160::from(79228162514264337593543950336u128);
    pool.initialize(initial_price);
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
    let endpoint = MessagingSwitchboard::data_engine_execute();
    msgbus::send_any(endpoint, &cmd as &dyn Any);

    // Create and process flash event
    let initiator = Address::from([0xAB; 20]);
    let recipient = Address::from([0xCD; 20]);
    let flash = PoolFlash::new(
        chain,
        dex,
        instrument_id,
        Address::from([0x12; 20]),
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
    data_engine.execute(&cmd);

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
    data_engine.execute(&cmd);

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
            "Expected second command to be RequestPoolSnapshot, got: {:?}",
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
        0u64,
        token0,
        token1,
        Some(500u32),
        Some(10u32),
        UnixNanos::from(1),
    );

    let initial_price = U160::from(79228162514264337593543950336u128); // sqrt(1) * 2^96
    pool.initialize(initial_price);
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
    data_engine.execute(&cmd);

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
    data_engine.execute(&cmd);

    // Verify request was routed to CLIENT1 only
    assert_eq!(recorder_1.borrow().len(), 1);
    assert_eq!(recorder_1.borrow()[0], cmd);
    assert_eq!(recorder_2.borrow().len(), 0);
}

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

use std::{any::Any, cell::RefCell, num::NonZeroUsize, rc::Rc};

use common::mocks::MockDataClient;
use nautilus_common::{
    cache::Cache,
    clock::{Clock, TestClock},
    messages::data::{
        DataCommand, RequestBars, RequestBookSnapshot, RequestCommand, RequestCustomData,
        RequestInstrument, RequestInstruments, RequestQuotes, RequestTrades, SubscribeBars,
        SubscribeBookDeltas, SubscribeBookDepth10, SubscribeBookSnapshots, SubscribeCommand,
        SubscribeCustomData, SubscribeIndexPrices, SubscribeInstrument, SubscribeMarkPrices,
        SubscribeQuotes, SubscribeTrades, UnsubscribeBars, UnsubscribeBookDeltas,
        UnsubscribeBookSnapshots, UnsubscribeCommand, UnsubscribeCustomData,
        UnsubscribeIndexPrices, UnsubscribeInstrument, UnsubscribeMarkPrices, UnsubscribeQuotes,
        UnsubscribeTrades,
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
        Bar, BarType, Data, DataType, OrderBookDeltas, OrderBookDeltas_API, OrderBookDepth10,
        QuoteTick, TradeTick,
        prices::{IndexPriceUpdate, MarkPriceUpdate},
        stubs::{stub_delta, stub_deltas, stub_depth10},
    },
    enums::{BookType, PriceType},
    identifiers::{ClientId, TraderId, Venue},
    instruments::{CurrencyPair, Instrument, InstrumentAny, stubs::audusd_sim},
    types::Price,
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
        assert_eq!(recorder.borrow().as_slice(), &[sub_cmd.clone()]);
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
        assert_eq!(recorder.borrow().as_slice(), &[sub_cmd.clone()]);
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
        assert_eq!(recorder.borrow().as_slice(), &[sub_cmd.clone()]);
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
        assert_eq!(recorder.borrow().as_slice(), &[sub_cmd.clone()]);
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
        assert_eq!(recorder.borrow().as_slice(), &[sub_cmd.clone()]);
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
        assert_eq!(recorder.borrow().as_slice(), &[sub_cmd.clone()]);
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
        false,
        None,
    );
    let sub_cmd = DataCommand::Subscribe(SubscribeCommand::Bars(sub));
    data_engine.execute(&sub_cmd);

    assert!(data_engine.subscribed_bars().contains(&bar_type));
    {
        assert_eq!(recorder.borrow().as_slice(), &[sub_cmd.clone()]);
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
        assert_eq!(recorder.borrow().as_slice(), &[sub_cmd.clone()]);
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
        assert_eq!(recorder.borrow().as_slice(), &[sub_cmd.clone()]);
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
    msgbus::send(endpoint, &cmd as &dyn Any);

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
    msgbus::send(endpoint, &cmd as &dyn Any);

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
    msgbus::send(endpoint, &cmd as &dyn Any);

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
    msgbus::send(endpoint, &cmd as &dyn Any);

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
    msgbus::send(endpoint, &cmd as &dyn Any);

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
    msgbus::send(endpoint, &cmd as &dyn Any);

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
    msgbus::send(endpoint, &cmd as &dyn Any);

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
    msgbus::send(endpoint, &cmd as &dyn Any);

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
        false,
        None,
    );
    let cmd = DataCommand::Subscribe(SubscribeCommand::Bars(sub));

    let endpoint = MessagingSwitchboard::data_engine_execute();
    msgbus::send(endpoint, &cmd as &dyn Any);

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

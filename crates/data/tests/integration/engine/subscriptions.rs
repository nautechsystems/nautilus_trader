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

use nautilus_common::{
    actor::DataActorCore, messages::data::IntoInstrumentIds, msgbus::ShareableMessageHandler,
};

use super::*;

#[rstest]
fn test_execute_subscribe_custom_data(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
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
    clock: Rc<RefCell<VirtualClock>>,
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
fn test_execute_subscribe_routes_to_default_client_when_no_client_id(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let mut data_engine = data_engine.borrow_mut();

    let broker_venue = Venue::new("IB");
    let broker_client_id = ClientId::new("IB");
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let client = MockDataClient::new_with_recorder(
        clock,
        cache,
        broker_client_id,
        Some(broker_venue),
        Some(Rc::clone(&recorder)),
    );

    let adapter = DataClientAdapter::new(
        broker_client_id,
        Some(broker_venue),
        true,
        true,
        Box::new(client),
    );
    data_engine.register_default_client(adapter);

    let sub_cmd = DataCommand::Subscribe(SubscribeCommand::BookDeltas(SubscribeBookDeltas::new(
        audusd_sim.id,
        BookType::L3_MBO,
        None,
        Some(audusd_sim.id.venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        true,
        None,
        None,
    )));
    data_engine.execute(sub_cmd.clone());

    assert_eq!(recorder.borrow().as_slice(), std::slice::from_ref(&sub_cmd));
}

#[rstest]
fn test_unsubscribe_book_deltas_removes_book_updater(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
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
    clock: Rc<RefCell<VirtualClock>>,
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
    let depth_topic = switchboard::get_book_depth_topic(audusd_sim.id);

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

    let sub_depth = DataCommand::Subscribe(SubscribeCommand::BookDepth(SubscribeBookDepth::new(
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
    assert_eq!(msgbus::subscriber_count_depth(depth_topic), 0);
    assert!(
        data_engine
            .cache()
            .borrow()
            .order_book(&audusd_sim.id)
            .is_none(),
        "unmanaged subscriptions must not auto-create an order book",
    );
}

#[rstest]
fn test_subscribe_book_deltas_composite_creates_books_per_underlying(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
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

    let mut data_engine = DataEngine::new(Rc::clone(&clock), Rc::clone(&cache), None);

    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
    register_mock_client(
        test_clock,
        Rc::clone(&cache),
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
fn test_reset_unsubscribes_composite_book_deltas(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let venue = Venue::new("XCME");

    let esz1 = make_es_future("ESZ1.XCME", "ESZ1");
    let esz1_id = esz1.id();

    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::FuturesContract(esz1))
        .unwrap();

    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);

    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
    register_mock_client(
        test_clock,
        Rc::clone(&cache),
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

    data_engine.process_data(Data::BookDelta(
        OrderBookDeltaTestBuilder::new(esz1_id).build(),
    ));
    let pre_reset_count = cache.borrow().order_book(&esz1_id).unwrap().update_count;
    assert_eq!(pre_reset_count, 1);

    data_engine.reset();

    data_engine.process_data(Data::BookDelta(
        OrderBookDeltaTestBuilder::new(esz1_id).build(),
    ));
    let post_reset_count = cache.borrow().order_book(&esz1_id).unwrap().update_count;
    assert_eq!(
        post_reset_count, pre_reset_count,
        "composite BookUpdater must be unsubscribed on reset; new deltas must not mutate the book",
    );
}

#[rstest]
fn test_unsubscribe_composite_keeps_overlapping_exact_alive(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
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

    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);

    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
    register_mock_client(
        test_clock,
        Rc::clone(&cache),
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

    data_engine.process_data(Data::BookDelta(
        OrderBookDeltaTestBuilder::new(esz1_id).build(),
    ));
    data_engine.process_data(Data::BookDelta(
        OrderBookDeltaTestBuilder::new(esh2_id).build(),
    ));

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
fn test_unsubscribe_composite_deltas_keeps_composite_depth_alive(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let venue = Venue::new("XCME");

    let esz1 = make_es_future("ESZ1.XCME", "ESZ1");
    let esz1_id = esz1.id();
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::FuturesContract(esz1))
        .unwrap();

    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
    register_mock_client(
        test_clock,
        Rc::clone(&cache),
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
            false,
            None,
            Some(parent_params()),
        ),
    )));
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::BookDepth(
        SubscribeBookDepth::new(
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
    let mut expected = OrderBook::new(esz1_id, BookType::L2_MBP);
    expected.apply_depth(&depth).unwrap();
    data_engine.process_data(Data::BookDepth(Box::new(depth)));

    let cache_view = cache.borrow();
    let esz1_book = cache_view
        .order_book(&esz1_id)
        .expect("ESZ1 book must exist while composite depth sub is active");
    assert_eq!(esz1_book.bids_as_map(None), expected.bids_as_map(None));
    assert_eq!(esz1_book.asks_as_map(None), expected.asks_as_map(None));
    assert_eq!(
        msgbus::subscriber_count_deltas(switchboard::get_book_deltas_topic(esz1_id)),
        0
    );
    assert_eq!(
        msgbus::subscriber_count_depth(switchboard::get_book_depth_topic(esz1_id)),
        1
    );
}

#[rstest]
fn test_unsubscribe_composite_deltas_keeps_exact_depth_handler_alive(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
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

    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
    register_mock_client(
        test_clock,
        Rc::clone(&cache),
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );

    let composite_id = InstrumentId::from("ES.FUT.XCME");
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::BookDepth(
        SubscribeBookDepth::new(
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
            false,
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

    data_engine.process_data(Data::BookDelta(
        OrderBookDeltaTestBuilder::new(esz1_id).build(),
    ));

    assert_eq!(cache.borrow().order_book(&esz1_id).unwrap().update_count, 0);
    let mut depth = stub_depth10();
    depth.instrument_id = esz1_id;
    let mut expected = OrderBook::new(esz1_id, BookType::L2_MBP);
    expected.apply_depth(&depth).unwrap();
    data_engine.process_data(Data::BookDepth(Box::new(depth)));

    let cache_view = cache.borrow();
    let book = cache_view.order_book(&esz1_id).unwrap();
    assert_eq!(book.bids_as_map(None), expected.bids_as_map(None));
    assert_eq!(book.asks_as_map(None), expected.asks_as_map(None));
    assert_eq!(
        msgbus::subscriber_count_deltas(switchboard::get_book_deltas_topic(esz1_id)),
        0
    );
    assert_eq!(
        msgbus::subscriber_count_depth(switchboard::get_book_depth_topic(esz1_id)),
        1
    );
}

#[rstest]
fn test_subscribe_book_deltas_composite_with_no_underlyings_is_noop(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let venue = Venue::new("XCME");

    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);

    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
    register_mock_client(
        test_clock,
        Rc::clone(&cache),
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
fn test_parent_subscribe_with_unparsable_id_returns_error(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let venue = Venue::new("BETFAIR");

    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
    register_mock_client(
        test_clock,
        Rc::clone(&cache),
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
        "rejected parent subscribe must NOT leave the id in book delta state",
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
fn test_snapshots_parent_subscribe_with_unparsable_id_returns_error(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let venue = Venue::new("BETFAIR");

    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
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
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
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

    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
    register_mock_client(
        test_clock,
        Rc::clone(&cache),
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
fn test_execute_subscribe_instrument(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
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
    clock: Rc<RefCell<VirtualClock>>,
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

macro_rules! batch_subscription_tests {
    ($validation_test:ident, $ownership_test:ident, $subscribe:ident, $unsubscribe:ident, $variant:ident, $subscribed:ident, $synthetic:ident, $mixed_error:literal) => {
        #[rstest]
        #[case::empty(Vec::new(), false, "instrument IDs cannot be empty")]
        #[case::duplicate(vec![InstrumentId::from("BTC/USD.SIM"), InstrumentId::from("BTC/USD.SIM")], false, "instrument IDs must be unique")]
        #[case::mixed_venues(vec![InstrumentId::from("BTC/USD.SIM"), InstrumentId::from("ETH/USD.OTHER")], false, "multiple instrument venues require an explicit client ID")]
        #[case::mixed_routes(vec![InstrumentId::from("BTC/USD.SIM"), InstrumentId::from("SYNTH.SYNTH")], true, $mixed_error)]
        fn $validation_test(
            #[case] ids: Vec<InstrumentId>,
            #[case] explicit_client: bool,
            #[case] error: &str,
            data_engine: Rc<RefCell<DataEngine>>,
            clock: Rc<RefCell<VirtualClock>>,
            cache: Rc<RefCell<Cache>>,
            client_id: ClientId,
        ) {
            let venue = Venue::from("SIM");
            let mut engine = data_engine.borrow_mut();
            let recorder = Rc::new(RefCell::new(Vec::new()));
            register_mock_client(clock, cache, client_id, venue, None, &recorder, &mut engine);

            let command = $subscribe {
                instrument_ids: ids,
                client_id: explicit_client.then_some(client_id),
                venue: Some(venue),
                command_id: UUID4::new(),
                ts_init: UnixNanos::from(47),
                correlation_id: Some(UUID4::new()),
                params: None,
            };

            let original = serde_json::to_value(&command).unwrap();

            let release =
                UnsubscribeCommand::$variant($unsubscribe {
                    instrument_ids: command.instrument_ids.clone(),
                    client_id: command.client_id,
                    venue: command.venue,
                    command_id: UUID4::new(),
                    ts_init: UnixNanos::from(53),
                    correlation_id: Some(UUID4::new()),
                    params: None,
                });

            let subscribe_error = engine
                .execute_subscribe(SubscribeCommand::$variant(command.clone()))
                .unwrap_err();
            let unsubscribe_error = engine.execute_unsubscribe(&release).unwrap_err();
            assert_eq!(subscribe_error.to_string(), error);
            assert_eq!(unsubscribe_error.to_string(), error);
            assert!(engine.$subscribed().is_empty());
            assert!(engine.$synthetic().is_empty());
            assert!(recorder.borrow().is_empty());
            assert_eq!(serde_json::to_value(command).unwrap(), original);
        }

        #[rstest]
        #[case::native(false)]
        #[case::external(true)]
        fn $ownership_test(
            #[case] external: bool,
            #[values(false, true)] same_params: bool,
            _stub_msgbus: Rc<RefCell<MessageBus>>,
            clock: Rc<RefCell<VirtualClock>>,
            cache: Rc<RefCell<Cache>>,
            client_id: ClientId,
            venue: Venue,
        ) {
            let config = DataEngineConfig {
                external_clients: external.then(|| vec![client_id]),
                ..DataEngineConfig::default()
            };

            let engine_clock = Rc::clone(&clock);
            let engine_cache = Rc::clone(&cache);
            let mut engine = DataEngine::new(engine_clock, engine_cache, Some(config));
            let recorder = Rc::new(RefCell::new(Vec::new()));
            if !external {
                register_mock_client(clock, cache, client_id, venue, None, &recorder, &mut engine);
            }

            let topic = format!("commands.data.{client_id}");
            let (subscribe_handler, subscribe_saver) =
                get_any_saving_handler::<SubscribeCommand>(None);
            let (unsubscribe_handler, unsubscribe_saver) =
                get_any_saving_handler::<UnsubscribeCommand>(None);
            msgbus::subscribe_any(topic.as_str().into(), subscribe_handler, None);
            msgbus::subscribe_any(topic.as_str().into(), unsubscribe_handler, None);
            let first = InstrumentId::from("BTC/USD.SIM");
            let second = InstrumentId::from("ETH/USD.SIM");
            let third = InstrumentId::from("SOL/USD.SIM");
            let first_params: Params =
                serde_json::from_value(serde_json::json!({"owner": 37})).unwrap();
            let second_params: Params = if same_params {
                first_params.clone()
            } else {
                serde_json::from_value(serde_json::json!({"owner": 59})).unwrap()
            };

            let first_command = $subscribe::new(
                [second, first, second],
                Some(client_id),
                Some(venue),
                UUID4::new(),
                UnixNanos::from(17),
                Some(UUID4::new()),
                Some(first_params),
            );

            let second_command = $subscribe::new(
                [first, third],
                Some(client_id),
                Some(venue),
                UUID4::new(),
                UnixNanos::from(23),
                Some(UUID4::new()),
                Some(second_params),
            );
            let original_first = serde_json::to_value(&first_command).unwrap();
            let original_second = serde_json::to_value(&second_command).unwrap();
            engine
                .execute_subscribe(SubscribeCommand::$variant(
                    first_command.clone(),
                ))
                .unwrap();
            engine
                .execute_subscribe(SubscribeCommand::$variant(
                    first_command.clone(),
                ))
                .unwrap();
            engine
                .execute_subscribe(SubscribeCommand::$variant(
                    second_command.clone(),
                ))
                .unwrap();

            let release = $unsubscribe::new(
                [second, first, third],
                Some(client_id),
                Some(venue),
                UUID4::new(),
                UnixNanos::from(31),
                Some(UUID4::new()),
                None,
            );
            let original_release = serde_json::to_value(&release).unwrap();
            engine
                .execute_unsubscribe(&UnsubscribeCommand::$variant(
                    release.clone(),
                ))
                .unwrap();

            if !external {
                assert_eq!(engine.$subscribed(), vec![first]);
            }

            let final_release = release.with_instrument_ids(first);
            engine
                .execute_unsubscribe(&UnsubscribeCommand::$variant(
                    final_release.clone(),
                ))
                .unwrap();
            let expected_subscribes = vec![
                SubscribeCommand::$variant(first_command.clone()),
                SubscribeCommand::$variant(
                    second_command.with_instrument_ids(third),
                ),
            ];
            let expected_releases = vec![
                SubscribeCommand::$variant(
                    first_command.with_instrument_ids(second),
                )
                .into_unsubscribe(
                    release.command_id,
                    release.ts_init,
                    release.correlation_id,
                ),
                SubscribeCommand::$variant(
                    second_command.with_instrument_ids(third),
                )
                .into_unsubscribe(
                    release.command_id,
                    release.ts_init,
                    release.correlation_id,
                ),
                SubscribeCommand::$variant(first_command.with_instrument_ids(first))
                    .into_unsubscribe(
                        final_release.command_id,
                        final_release.ts_init,
                        final_release.correlation_id,
                    ),
            ];

            if external {
                assert_eq!(
                    serde_json::to_value(subscribe_saver.get_messages()).unwrap(),
                    serde_json::to_value(expected_subscribes).unwrap()
                );
                assert_eq!(
                    serde_json::to_value(unsubscribe_saver.get_messages()).unwrap(),
                    serde_json::to_value(expected_releases).unwrap()
                );
            } else {
                let expected: Vec<_> = expected_subscribes
                    .into_iter()
                    .map(DataCommand::Subscribe)
                    .chain(expected_releases.into_iter().map(DataCommand::Unsubscribe))
                    .collect();

                let serialize = |commands: &[DataCommand]| {
                    commands
                        .iter()
                        .map(|command| match command {
                            DataCommand::Subscribe(command) => {
                                serde_json::to_value(command).unwrap()
                            }
                            DataCommand::Unsubscribe(command) => {
                                serde_json::to_value(command).unwrap()
                            }
                            _ => panic!("unexpected subscription command {command:?}"),
                        })
                        .collect::<Vec<_>>()
                };

                assert_eq!(serialize(&recorder.borrow()), serialize(&expected));
                assert_eq!(
                    engine.$subscribed(),
                    Vec::<InstrumentId>::new()
                );
            }

            assert_eq!(serde_json::to_value(first_command).unwrap(), original_first);
            assert_eq!(
                serde_json::to_value(second_command).unwrap(),
                original_second
            );
            assert_eq!(serde_json::to_value(release).unwrap(), original_release);
        }
    };
}

batch_subscription_tests!(
    test_quote_batches_reject_invalid_intents_before_effects,
    test_quote_batches_filter_dispatch_and_preserve_each_owner,
    SubscribeQuotes,
    UnsubscribeQuotes,
    Quotes,
    subscribed_quotes,
    subscribed_synthetic_quotes,
    "quote identifier lists cannot mix client, synthetic, and spread routes"
);

batch_subscription_tests!(
    test_trade_batches_reject_invalid_intents_before_effects,
    test_trade_batches_filter_dispatch_and_preserve_each_owner,
    SubscribeTrades,
    UnsubscribeTrades,
    Trades,
    subscribed_trades,
    subscribed_synthetic_trades,
    "trade identifier lists cannot mix client and synthetic routes"
);

#[rstest]
fn test_unsubscribe_quotes_keeps_client_subscribed_until_final_owner(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
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

    let sub_a = SubscribeQuotes::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let sub_cmd_a = DataCommand::Subscribe(SubscribeCommand::Quotes(sub_a));
    data_engine.execute(sub_cmd_a.clone());

    let sub_b = SubscribeQuotes::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Quotes(sub_b)));

    let unsub_a = UnsubscribeQuotes::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    data_engine.execute(DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(
        unsub_a,
    )));

    assert_eq!(
        recorder.borrow().as_slice(),
        std::slice::from_ref(&sub_cmd_a)
    );

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

    let unsub_b = UnsubscribeQuotes::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );
    let unsub_cmd_b = DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(unsub_b));
    data_engine.execute(unsub_cmd_b.clone());

    assert_eq!(recorder.borrow().as_slice(), &[sub_cmd_a, unsub_cmd_b]);
}

#[rstest]
fn test_unsubscribe_quotes_ignores_wildcard_observers(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
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
    clock: Rc<RefCell<VirtualClock>>,
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
    clock: Rc<RefCell<VirtualClock>>,
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
fn test_execute_subscribe_internal_bars_stays_local(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
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
    let trade_topic = switchboard::get_trades_topic(bar_type.instrument_id());
    let subscribe_command_id = UUID4::new();

    let sub = SubscribeBars::new(
        bar_type,
        Some(client_id),
        Some(venue),
        subscribe_command_id,
        UnixNanos::default(),
        None,
        None,
    );
    let sub_cmd = DataCommand::Subscribe(SubscribeCommand::Bars(sub));
    data_engine.execute(sub_cmd);

    assert_eq!(msgbus::exact_subscriber_count_trades(trade_topic), 1);
    {
        let recorded = recorder.borrow();
        assert_eq!(recorded.len(), 1);

        match &recorded[0] {
            DataCommand::Subscribe(SubscribeCommand::Trades(cmd)) => {
                assert_eq!(
                    cmd.single_instrument_id().unwrap(),
                    bar_type.instrument_id()
                );
                assert_eq!(cmd.correlation_id, Some(subscribe_command_id));
            }
            other => panic!("expected source trade subscription, was {other:?}"),
        }
    }

    let unsubscribe_command_id = UUID4::new();

    let unsub = UnsubscribeBars::new(
        bar_type,
        Some(client_id),
        Some(venue),
        unsubscribe_command_id,
        UnixNanos::default(),
        None,
        None,
    );
    let unsub_cmd = DataCommand::Unsubscribe(UnsubscribeCommand::Bars(unsub));
    data_engine.execute(unsub_cmd);

    assert_eq!(audusd_sim.id(), bar_type.instrument_id());
    assert_eq!(msgbus::exact_subscriber_count_trades(trade_topic), 0);
    {
        let recorded = recorder.borrow();
        assert_eq!(recorded.len(), 2);

        match &recorded[1] {
            DataCommand::Unsubscribe(UnsubscribeCommand::Trades(cmd)) => {
                assert_eq!(
                    cmd.single_instrument_id().unwrap(),
                    bar_type.instrument_id()
                );
                assert_eq!(cmd.correlation_id, Some(unsubscribe_command_id));
            }
            other => panic!("expected source trade unsubscription, was {other:?}"),
        }
    }
}

#[rstest]
fn test_unsubscribe_internal_bars_stays_local_with_remaining_exact_subscribers(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    // Internal aggregation is local to the engine, and exact subscribers keep the
    // aggregator active without forwarding to the client.
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
    msgbus::subscribe_bars(bar_topic.into(), handler.clone(), None);

    let subscribe_command_id = UUID4::new();
    let sub_cmd = DataCommand::Subscribe(SubscribeCommand::Bars(SubscribeBars::new(
        bar_type,
        Some(client_id),
        Some(venue),
        subscribe_command_id,
        UnixNanos::default(),
        None,
        None,
    )));
    data_engine.execute(sub_cmd);

    let unsubscribe_command_id = UUID4::new();
    let unsub_cmd = DataCommand::Unsubscribe(UnsubscribeCommand::Bars(UnsubscribeBars::new(
        bar_type,
        Some(client_id),
        Some(venue),
        unsubscribe_command_id,
        UnixNanos::default(),
        None,
        None,
    )));
    data_engine.execute(unsub_cmd);

    {
        let recorded = recorder.borrow();
        assert_eq!(recorded.len(), 1);

        match &recorded[0] {
            DataCommand::Subscribe(SubscribeCommand::Trades(cmd)) => {
                assert_eq!(
                    cmd.single_instrument_id().unwrap(),
                    bar_type.instrument_id()
                );
                assert_eq!(cmd.correlation_id, Some(subscribe_command_id));
            }
            other => panic!("expected source trade subscription, was {other:?}"),
        }
    }

    msgbus::unsubscribe_bars(bar_topic.into(), &handler);
    let fallback_client_id = ClientId::new("FALLBACK-CLIENT");
    data_engine.execute(DataCommand::Unsubscribe(UnsubscribeCommand::Bars(
        UnsubscribeBars::new(
            bar_type,
            Some(fallback_client_id),
            Some(venue),
            unsubscribe_command_id,
            UnixNanos::default(),
            None,
            None,
        ),
    )));

    {
        let recorded = recorder.borrow();
        assert_eq!(recorded.len(), 2);

        let DataCommand::Unsubscribe(UnsubscribeCommand::Trades(command)) = &recorded[1] else {
            panic!("expected source trade unsubscribe, was {:?}", recorded[1]);
        };

        assert_eq!(command.client_id, Some(client_id));
        assert_eq!(command.correlation_id, Some(unsubscribe_command_id));
    }
}

#[rstest]
fn test_external_client_internal_bar_subscription_skips_local_aggregator(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let instrument = InstrumentAny::CurrencyPair(audusd_sim);
    cache.borrow_mut().add_instrument(instrument).unwrap();

    let config = DataEngineConfig {
        external_clients: Some(vec![client_id]),
        ..DataEngineConfig::default()
    };

    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), Some(config));

    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
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

    let bar_type = BarType::from("AUD/USD.SIM-1-MINUTE-LAST-INTERNAL");
    let trade_topic = switchboard::get_trades_topic(bar_type.instrument_id());

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

    assert_eq!(msgbus::exact_subscriber_count_trades(trade_topic), 0);
    assert_eq!(recorder.borrow().as_slice(), &[]);
}

#[rstest]
fn test_external_client_subscribe_registers_streamable_payload_types(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let config = DataEngineConfig {
        external_clients: Some(vec![client_id]),
        ..DataEngineConfig::default()
    };

    let mut data_engine = DataEngine::new(clock, cache, Some(config));

    for (name, cmd, expected) in streamable_subscribe_cases(audusd_sim.id, client_id, venue) {
        stub_msgbus.borrow_mut().clear_streaming_types();
        data_engine.execute(DataCommand::Subscribe(cmd));

        assert_only_streaming_type(&stub_msgbus.borrow(), expected, name);
    }
}

#[rstest]
fn test_external_client_releases_after_final_owner(
    #[values(false, true)] stopped: bool,
    audusd_sim: CurrencyPair,
    _stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let config = DataEngineConfig {
        external_clients: Some(vec![client_id]),
        ..DataEngineConfig::default()
    };

    let mut data_engine = DataEngine::new(clock, cache, Some(config));
    let topic = format!("commands.data.{client_id}");
    let first_params = Params::new([("owner".to_string(), serde_json::json!(1))].into());
    let second_params = Params::new([("owner".to_string(), serde_json::json!(2))].into());
    let first_subscribe = SubscribeCommand::Quotes(SubscribeQuotes::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::from(1),
        None,
        Some(first_params.clone()),
    ));
    let second_subscribe = SubscribeCommand::Quotes(SubscribeQuotes::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::from(2),
        None,
        Some(second_params),
    ));
    let (subscribe_handler, subscribe_saver) = get_any_saving_handler::<SubscribeCommand>(None);
    msgbus::subscribe_any(topic.as_str().into(), subscribe_handler, None);

    data_engine.execute(DataCommand::Subscribe(first_subscribe.clone()));

    if stopped {
        data_engine.stop();
    }

    data_engine.execute(DataCommand::Subscribe(second_subscribe));

    assert_eq!(
        serde_json::to_value(subscribe_saver.get_messages()).unwrap(),
        serde_json::to_value([first_subscribe.clone()]).unwrap(),
    );

    let (unsubscribe_handler, unsubscribe_saver) =
        get_any_saving_handler::<UnsubscribeCommand>(None);
    msgbus::subscribe_any(topic.as_str().into(), unsubscribe_handler, None);
    data_engine.execute(DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(
        UnsubscribeQuotes::new(
            audusd_sim.id,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::from(3),
            None,
            None,
        ),
    )));
    assert!(unsubscribe_saver.get_messages().is_empty());

    let final_command_id = UUID4::new();
    data_engine.execute(DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(
        UnsubscribeQuotes::new(
            audusd_sim.id,
            Some(client_id),
            Some(venue),
            final_command_id,
            UnixNanos::from(4),
            None,
            None,
        ),
    )));

    let commands = unsubscribe_saver.get_messages();

    let [UnsubscribeCommand::Quotes(command)] = commands.as_slice() else {
        panic!("expected one final external unsubscribe, was {commands:?}");
    };

    assert_eq!(command.instrument_ids, vec![audusd_sim.id]);
    assert_eq!(command.client_id, Some(client_id));
    assert_eq!(command.venue, Some(venue));
    assert_eq!(command.command_id, final_command_id);
    assert_eq!(command.ts_init, UnixNanos::from(4));
    assert_eq!(command.params.as_ref(), Some(&first_params));

    if stopped {
        let subscribe = first_subscribe
            .with_instrument_ids(InstrumentId::from("EUR/USD.SIM"))
            .unwrap();
        data_engine.execute(DataCommand::Subscribe(subscribe.clone()));
        assert_eq!(subscribe_saver.get_messages().len(), 1);
        data_engine.start();
        data_engine.execute(DataCommand::Subscribe(subscribe));
        assert_eq!(subscribe_saver.get_messages().len(), 2);
    }
}

#[rstest]
fn test_regular_client_subscribe_does_not_register_streaming_payload_type(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);

    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
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

    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Quotes(
        SubscribeQuotes::new(
            audusd_sim.id,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ),
    )));

    assert_no_streaming_types(&stub_msgbus.borrow(), "regular quote subscribe");
    assert_eq!(recorder.borrow().len(), 1);
}

#[rstest]
fn test_external_client_subscribe_keeps_non_streamable_payload_types_closed(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let config = DataEngineConfig {
        external_clients: Some(vec![client_id]),
        ..DataEngineConfig::default()
    };

    let mut data_engine = DataEngine::new(clock, cache, Some(config));

    for (name, cmd) in non_streamable_subscribe_cases(audusd_sim.id, client_id, venue) {
        stub_msgbus.borrow_mut().clear_streaming_types();
        data_engine.execute(DataCommand::Subscribe(cmd));

        assert_no_streaming_types(&stub_msgbus.borrow(), name);
    }
}

#[rstest]
fn test_bar_aggregator_quote_subscription_priority_is_between_4_and_6(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
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

    let order_high = Rc::clone(&dispatch_order);

    let handler_high = TypedHandler::from_with_id("prio-6", move |_q: &QuoteTick| {
        order_high.borrow_mut().push("high");
    });

    msgbus::subscribe_quotes(quote_topic.into(), handler_high, Some(6));

    let order_low = Rc::clone(&dispatch_order);

    let handler_low = TypedHandler::from_with_id("prio-4", move |_q: &QuoteTick| {
        order_low.borrow_mut().push("low");
    });

    msgbus::subscribe_quotes(quote_topic.into(), handler_low, Some(4));

    let order_bar = Rc::clone(&dispatch_order);

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
    clock: Rc<RefCell<VirtualClock>>,
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

    let order_high = Rc::clone(&dispatch_order);

    let handler_high = TypedHandler::from_with_id("prio-6", move |_t: &TradeTick| {
        order_high.borrow_mut().push("high");
    });

    msgbus::subscribe_trades(trades_topic.into(), handler_high, Some(6));

    let order_low = Rc::clone(&dispatch_order);

    let handler_low = TypedHandler::from_with_id("prio-4", move |_t: &TradeTick| {
        order_low.borrow_mut().push("low");
    });

    msgbus::subscribe_trades(trades_topic.into(), handler_low, Some(4));

    let order_bar = Rc::clone(&dispatch_order);

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
        AggressorSide::Buy,
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
    clock: Rc<RefCell<VirtualClock>>,
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
    // Aggregated bars are emitted with the standard bar type (v1 parity)
    let target_topic = switchboard::get_bars_topic(bar_type.standard());

    let dispatch_order: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));

    let order_high = Rc::clone(&dispatch_order);

    let handler_high = TypedHandler::from_with_id("prio-1", move |_b: &Bar| {
        order_high.borrow_mut().push("high");
    });

    msgbus::subscribe_bars(source_topic.into(), handler_high, Some(1));

    let order_bar = Rc::clone(&dispatch_order);

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
    clock: Rc<RefCell<VirtualClock>>,
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
fn test_unsubscribe_mark_prices_keeps_client_subscribed_until_final_owner(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
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

    let sub_cmd_a = DataCommand::Subscribe(SubscribeCommand::MarkPrices(SubscribeMarkPrices::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )));
    data_engine.execute(sub_cmd_a.clone());
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::MarkPrices(
        SubscribeMarkPrices::new(
            audusd_sim.id,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ),
    )));

    data_engine.execute(DataCommand::Unsubscribe(UnsubscribeCommand::MarkPrices(
        UnsubscribeMarkPrices::new(
            audusd_sim.id,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ),
    )));

    assert_eq!(
        recorder.borrow().as_slice(),
        std::slice::from_ref(&sub_cmd_a)
    );

    let unsub_cmd_b =
        DataCommand::Unsubscribe(UnsubscribeCommand::MarkPrices(UnsubscribeMarkPrices::new(
            audusd_sim.id,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        )));
    data_engine.execute(unsub_cmd_b.clone());

    assert_eq!(recorder.borrow().as_slice(), &[sub_cmd_a, unsub_cmd_b]);
}

#[rstest]
fn test_unsubscribe_mark_prices_ignores_wildcard_observers(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
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
    clock: Rc<RefCell<VirtualClock>>,
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
    clock: Rc<RefCell<VirtualClock>>,
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
    clock: Rc<RefCell<VirtualClock>>,
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
    clock: Rc<RefCell<VirtualClock>>,
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
    clock: Rc<RefCell<VirtualClock>>,
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
    clock: Rc<RefCell<VirtualClock>>,
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
    clock: Rc<RefCell<VirtualClock>>,
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
    clock: Rc<RefCell<VirtualClock>>,
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
fn test_synthetic_quote_subscription_publishes_from_component_quotes(
    stub_msgbus: Rc<RefCell<MessageBus>>,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);

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
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);

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
        AggressorSide::Buy,
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
        AggressorSide::Sell,
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
fn test_duplicate_synthetic_quote_subscription_publishes_once(
    stub_msgbus: Rc<RefCell<MessageBus>>,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);

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
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);

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
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);

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
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);

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
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
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
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);

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
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);

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
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);

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
fn test_synthetic_quotes_release_after_final_owner(
    #[values(false, true)] stopped: bool,
    stub_msgbus: Rc<RefCell<MessageBus>>,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);
    let (synthetic, _, _) = synthetic_index();
    let synthetic_id = synthetic.id;
    cache.borrow_mut().add_synthetic(synthetic).unwrap();

    data_engine.execute(subscribe_synthetic_quotes_cmd(synthetic_id));

    if stopped {
        data_engine.stop();
    }

    data_engine.execute(subscribe_synthetic_quotes_cmd(synthetic_id));
    data_engine.execute(unsubscribe_synthetic_quotes_cmd(synthetic_id));

    assert!(
        data_engine
            .subscribed_synthetic_quotes()
            .contains(&synthetic_id)
    );

    data_engine.execute(unsubscribe_synthetic_quotes_cmd(synthetic_id));

    assert!(
        !data_engine
            .subscribed_synthetic_quotes()
            .contains(&synthetic_id)
    );

    if stopped {
        data_engine.execute(subscribe_synthetic_quotes_cmd(synthetic_id));
        assert!(data_engine.subscribed_synthetic_quotes().is_empty());
        data_engine.start();
        assert!(data_engine.subscribed_synthetic_quotes().is_empty());
    }
}

#[rstest]
fn test_synthetic_trades_release_after_final_owner(
    #[values(false, true)] batch: bool,
    #[values(false, true)] stopped: bool,
    stub_msgbus: Rc<RefCell<MessageBus>>,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);
    let (synthetic, component_a, component_b) = synthetic_index();
    let synthetic_id = synthetic.id;
    cache.borrow_mut().add_synthetic(synthetic).unwrap();
    let mut instrument_ids = vec![synthetic_id];

    if batch {
        let synthetic = synthetic_index_with_components("BTC-ETH-SECOND", component_a, component_b);
        instrument_ids.push(synthetic.id);
        cache.borrow_mut().add_synthetic(synthetic).unwrap();
    }

    instrument_ids.sort();

    data_engine.execute(subscribe_synthetic_trades_cmd(&instrument_ids));

    if stopped {
        data_engine.stop();
    }

    data_engine.execute(subscribe_synthetic_trades_cmd(&instrument_ids));
    data_engine.execute(unsubscribe_synthetic_trades_cmd(&instrument_ids));

    let mut subscribed = data_engine.subscribed_synthetic_trades();
    subscribed.sort();
    assert_eq!(subscribed, instrument_ids);

    data_engine.execute(unsubscribe_synthetic_trades_cmd(&instrument_ids));

    assert_eq!(
        data_engine.subscribed_synthetic_trades(),
        Vec::<InstrumentId>::new(),
    );

    if stopped {
        data_engine.execute(subscribe_synthetic_trades_cmd(&instrument_ids));
        assert!(data_engine.subscribed_synthetic_trades().is_empty());
        data_engine.start();
        assert!(data_engine.subscribed_synthetic_trades().is_empty());
    }
}

#[cfg(feature = "defi")]
#[rstest]
#[case::running(false, false)]
#[case::queued_after_stop(true, false)]
#[case::direct_after_stop(true, true)]
fn test_execute_subscribe_blocks(
    #[case] stopped: bool,
    #[case] direct: bool,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
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

    let sub = SubscribeBlocks {
        chain: blockchain,
        client_id: Some(client_id),
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
    };
    let sub_cmd = DataCommand::DefiSubscribe(DefiSubscribeCommand::Blocks(sub.clone()));

    data_engine.execute(sub_cmd.clone());

    assert!(data_engine.subscribed_blocks().contains(&blockchain));
    {
        assert_eq!(recorder.borrow().as_slice(), std::slice::from_ref(&sub_cmd));
    }

    if stopped {
        data_engine.stop();
        let temporary = DefiSubscribeCommand::Blocks(SubscribeBlocks {
            command_id: UUID4::new(),
            ..sub.clone()
        });

        for _ in 0..2 {
            if direct {
                data_engine
                    .execute_defi_subscribe(temporary.clone())
                    .unwrap();
            } else {
                data_engine.execute(DataCommand::DefiSubscribe(temporary.clone()));
            }
        }

        data_engine
            .execute_defi_subscribe(DefiSubscribeCommand::Blocks(SubscribeBlocks {
                chain: Blockchain::Arbitrum,
                command_id: UUID4::new(),
                ..sub
            }))
            .unwrap();
        data_engine.execute(DataCommand::DefiUnsubscribe(
            temporary.into_unsubscribe(UUID4::new(), UnixNanos::default()),
        ));
        assert_eq!(data_engine.subscribed_blocks(), vec![blockchain]);
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
    clock: Rc<RefCell<VirtualClock>>,
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
fn test_execute_subscribe_pool_liquidity_updates(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
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
    clock: Rc<RefCell<VirtualClock>>,
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
    clock: Rc<RefCell<VirtualClock>>,
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

#[rstest]
fn test_subscribed_book_snapshots_preserve_subscription_order(
    audusd_sim: CurrencyPair,
    gbpusd_sim: CurrencyPair,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    // Pin IndexMap iteration on DataEngine.book_snapshot_counts: the per-tick
    // BookSnapshotter publishes in iteration order, and the public
    // subscribed_book_snapshots() Vec must reflect subscription order across runs.
    let data_engine = create_snapshot_test_engine(Rc::clone(&clock), Rc::clone(&cache));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        Rc::clone(&cache),
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
fn test_duplicate_book_snapshot_subscriptions_require_matching_unsubscribes(
    audusd_sim: CurrencyPair,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let data_engine = create_snapshot_test_engine(Rc::clone(&clock), Rc::clone(&cache));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        Rc::clone(&clock),
        Rc::clone(&cache),
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
        DataCommand::Unsubscribe(UnsubscribeCommand::BookDeltas(cmd)) if cmd.single_instrument_id().unwrap() == audusd_sim.id
    ));
    drop(recorded);

    let snapshot_count = saver.get_messages().len();
    process_book_delta(&data_engine, audusd_sim.id);
    advance_clock_and_dispatch(&clock, 200_000_000);
    assert_eq!(saver.get_messages().len(), snapshot_count);
}

#[rstest]
fn test_unsubscribe_book_snapshots_during_publish_does_not_panic(
    audusd_sim: CurrencyPair,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let data_engine = create_snapshot_test_engine(Rc::clone(&clock), Rc::clone(&cache));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        Rc::clone(&clock),
        Rc::clone(&cache),
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
    let snapshot_count_clone = Rc::clone(&snapshot_count);
    let data_engine_clone = Rc::clone(&data_engine);

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
        DataCommand::Unsubscribe(UnsubscribeCommand::BookDeltas(cmd)) if cmd.single_instrument_id().unwrap() == audusd_sim.id
    ));
    drop(recorded);

    process_book_delta(&data_engine, audusd_sim.id);
    advance_clock_and_dispatch(&clock, 200_000_000);
    assert_eq!(*snapshot_count.borrow(), 1);
}

#[rstest]
fn test_unsubscribe_book_deltas_keeps_snapshot_subscriptions_active(
    audusd_sim: CurrencyPair,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let data_engine = create_snapshot_test_engine(Rc::clone(&clock), Rc::clone(&cache));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        Rc::clone(&clock),
        Rc::clone(&cache),
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

    assert_eq!(
        book_deltas_subscribe_count(&recorder.borrow(), audusd_sim.id),
        1,
        "snapshot and direct book-delta subscribers must share one physical deltas feed",
    );

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

#[rstest]
fn test_duplicate_book_deltas_unsubscribe_keeps_remaining_subscription_active(
    audusd_sim: CurrencyPair,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let data_engine = create_snapshot_test_engine(Rc::clone(&clock), Rc::clone(&cache));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        Rc::clone(&cache),
        client_id,
        venue,
        Some(venue),
        &recorder,
        &mut data_engine.borrow_mut(),
    );

    let _ = cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim.clone()));

    execute_book_delta_subscribe(&data_engine, audusd_sim.id, client_id, venue);
    execute_book_delta_subscribe(&data_engine, audusd_sim.id, client_id, venue);

    assert_eq!(
        book_deltas_subscribe_count(&recorder.borrow(), audusd_sim.id),
        1,
        "duplicate logical book-delta subscribers must share one physical deltas feed",
    );

    execute_book_delta_unsubscribe(&data_engine, audusd_sim.id, client_id, venue);

    assert_eq!(
        book_deltas_unsubscribe_count(&recorder.borrow(), audusd_sim.id),
        0,
        "unsubscribing one logical book-delta owner must keep the physical feed active",
    );

    process_book_delta(&data_engine, audusd_sim.id);
    let update_count = cache
        .borrow()
        .order_book(&audusd_sim.id)
        .expect("book must exist while one logical subscriber remains")
        .update_count;

    execute_book_delta_unsubscribe(&data_engine, audusd_sim.id, client_id, venue);

    assert_eq!(
        book_deltas_unsubscribe_count(&recorder.borrow(), audusd_sim.id),
        1,
        "the physical deltas feed should unsubscribe after the last logical owner leaves",
    );

    process_book_delta(&data_engine, audusd_sim.id);

    assert_eq!(
        cache
            .borrow()
            .order_book(&audusd_sim.id)
            .expect("book remains in cache after updater teardown")
            .update_count,
        update_count,
        "deltas published after the last unsubscribe must not reach the torn-down updater",
    );
}

#[rstest]
fn test_distinct_book_deltas_keys_share_physical_subscription(
    audusd_sim: CurrencyPair,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let data_engine = create_snapshot_test_engine(Rc::clone(&clock), Rc::clone(&cache));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        Rc::clone(&cache),
        client_id,
        venue,
        Some(venue),
        &recorder,
        &mut data_engine.borrow_mut(),
    );

    let _ = cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim.clone()));

    execute_book_delta_subscribe_for_route(
        &data_engine,
        audusd_sim.id,
        Some(client_id),
        Some(venue),
    );
    execute_book_delta_subscribe_for_route(&data_engine, audusd_sim.id, None, Some(venue));

    assert_eq!(
        book_deltas_subscribe_count(&recorder.borrow(), audusd_sim.id),
        1,
        "distinct logical book-delta keys routed to one client must share the physical feed",
    );

    execute_book_delta_unsubscribe_for_route(
        &data_engine,
        audusd_sim.id,
        Some(client_id),
        Some(venue),
    );

    assert_eq!(
        book_deltas_unsubscribe_count(&recorder.borrow(), audusd_sim.id),
        0,
        "unsubscribing one routed book-delta key must keep the shared physical feed active",
    );

    process_book_delta(&data_engine, audusd_sim.id);
    let update_count = cache
        .borrow()
        .order_book(&audusd_sim.id)
        .expect("book must exist while one routed subscriber remains")
        .update_count;

    execute_book_delta_unsubscribe_for_route(&data_engine, audusd_sim.id, None, Some(venue));

    assert_eq!(
        book_deltas_unsubscribe_count(&recorder.borrow(), audusd_sim.id),
        1,
        "the shared physical feed should unsubscribe after all routed keys leave",
    );

    process_book_delta(&data_engine, audusd_sim.id);

    assert_eq!(
        cache
            .borrow()
            .order_book(&audusd_sim.id)
            .expect("book remains in cache after updater teardown")
            .update_count,
        update_count,
        "deltas published after all routed keys leave must not reach the torn-down updater",
    );
}

#[rstest]
#[case::instrument("instrument")]
#[case::status("status")]
#[case::close("close")]
#[case::greeks("greeks")]
fn test_subscribe_synthetic_instrument_rejected(
    #[case] variant: &str,
    #[values(false, true)] stopped: bool,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
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

    if stopped {
        data_engine.stop();
    }

    let synth_id = synthetic_instrument_id();
    let cmd = build_synthetic_subscribe(variant, synth_id, client_id, venue, UnixNanos::default());
    data_engine.execute(cmd);

    if stopped {
        data_engine.start();
    }

    assert!(
        recorder.borrow().is_empty(),
        "Synthetic subscribe ({variant}) must not reach the client, received {:?}",
        recorder.borrow()
    );
    assert_eq!(
        data_engine.command_count(),
        u64::from(!stopped),
        "Only running commands count as executed"
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
    clock: Rc<RefCell<VirtualClock>>,
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
fn test_quote_routes_release_independently_with_shared_topic(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let first_client_id = ClientId::new("FIRST-CLIENT");
    let second_client_id = ClientId::new("SECOND-CLIENT");
    let first_recorder = Rc::new(RefCell::new(Vec::new()));
    let second_recorder = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        Rc::clone(&clock),
        Rc::clone(&cache),
        first_client_id,
        venue,
        None,
        &first_recorder,
        &mut data_engine,
    );
    register_mock_client(
        clock,
        cache,
        second_client_id,
        venue,
        None,
        &second_recorder,
        &mut data_engine,
    );
    let instrument_id = audusd_sim.id;
    data_engine.process(&InstrumentAny::CurrencyPair(audusd_sim) as &dyn Any);

    for client_id in [first_client_id, second_client_id] {
        data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Quotes(
            SubscribeQuotes::new(
                instrument_id,
                Some(client_id),
                Some(venue),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ),
        )));
    }

    let topic = switchboard::get_quotes_topic(instrument_id);
    let (remaining_handler, _saver) = get_typed_message_saving_handler::<QuoteTick>(Some(
        Ustr::from("remaining-route-subscriber"),
    ));
    msgbus::subscribe_quotes(topic.into(), remaining_handler.clone(), None);

    data_engine.execute(DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(
        UnsubscribeQuotes::new(
            instrument_id,
            Some(first_client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ),
    )));

    assert_eq!(first_recorder.borrow().len(), 2);
    assert!(matches!(
        &first_recorder.borrow()[1],
        DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(command))
            if command.client_id == Some(first_client_id)
    ));
    assert_eq!(second_recorder.borrow().len(), 1);

    msgbus::unsubscribe_quotes(topic.into(), &remaining_handler);
    data_engine.execute(DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(
        UnsubscribeQuotes::new(
            instrument_id,
            Some(second_client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ),
    )));

    assert_eq!(second_recorder.borrow().len(), 2);
    assert!(matches!(
        &second_recorder.borrow()[1],
        DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(command))
            if command.client_id == Some(second_client_id)
    ));
}

#[rstest]
fn test_subscribed_bars_includes_internal_aggregations(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
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

    // Internally aggregated subscriptions never reach a client, but must still
    // be reported (v1 parity)
    assert!(data_engine.subscribed_bars().contains(&bar_type));
}

fn streamable_subscribe_cases(
    instrument_id: InstrumentId,
    client_id: ClientId,
    venue: Venue,
) -> Vec<(&'static str, SubscribeCommand, BusPayloadType)> {
    let data_type = DataType::new("RustTestCustomData", None, None);
    let bar_type = BarType::from(format!("{instrument_id}-1-MINUTE-LAST-EXTERNAL").as_str());
    let interval_ms = NonZeroUsize::new(1_000).expect("interval must be non-zero");

    vec![
        (
            "custom data",
            SubscribeCommand::Data(SubscribeCustomData::new(
                Some(client_id),
                Some(venue),
                data_type,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            )),
            BusPayloadType::Custom(Ustr::from("RustTestCustomData")),
        ),
        (
            "instrument",
            SubscribeCommand::Instrument(SubscribeInstrument::new(
                instrument_id,
                Some(client_id),
                Some(venue),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            )),
            BusPayloadType::Instrument,
        ),
        (
            "instruments",
            SubscribeCommand::Instruments(SubscribeInstruments::new(
                Some(client_id),
                venue,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            )),
            BusPayloadType::Instrument,
        ),
        (
            "book deltas",
            SubscribeCommand::BookDeltas(SubscribeBookDeltas::new(
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
            )),
            BusPayloadType::OrderBookDeltas,
        ),
        (
            "book snapshots",
            SubscribeCommand::BookSnapshots(SubscribeBookSnapshots::new(
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
            )),
            BusPayloadType::OrderBookDeltas,
        ),
        (
            "book depth",
            SubscribeCommand::BookDepth(SubscribeBookDepth::new(
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
            )),
            BusPayloadType::OrderBookDepth,
        ),
        (
            "quotes",
            SubscribeCommand::Quotes(SubscribeQuotes::new(
                instrument_id,
                Some(client_id),
                Some(venue),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            )),
            BusPayloadType::QuoteTick,
        ),
        (
            "trades",
            SubscribeCommand::Trades(SubscribeTrades::new(
                instrument_id,
                Some(client_id),
                Some(venue),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            )),
            BusPayloadType::TradeTick,
        ),
        (
            "bars",
            SubscribeCommand::Bars(SubscribeBars::new(
                bar_type,
                Some(client_id),
                Some(venue),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            )),
            BusPayloadType::Bar,
        ),
        (
            "mark prices",
            SubscribeCommand::MarkPrices(SubscribeMarkPrices::new(
                instrument_id,
                Some(client_id),
                Some(venue),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            )),
            BusPayloadType::MarkPriceUpdate,
        ),
        (
            "index prices",
            SubscribeCommand::IndexPrices(SubscribeIndexPrices::new(
                instrument_id,
                Some(client_id),
                Some(venue),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            )),
            BusPayloadType::IndexPriceUpdate,
        ),
        (
            "funding rates",
            SubscribeCommand::FundingRates(SubscribeFundingRates::new(
                instrument_id,
                Some(client_id),
                Some(venue),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            )),
            BusPayloadType::FundingRateUpdate,
        ),
        (
            "option greeks",
            SubscribeCommand::OptionGreeks(SubscribeOptionGreeks::new(
                instrument_id,
                Some(client_id),
                Some(venue),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            )),
            BusPayloadType::OptionGreeks,
        ),
    ]
}

fn non_streamable_subscribe_cases(
    instrument_id: InstrumentId,
    client_id: ClientId,
    venue: Venue,
) -> Vec<(&'static str, SubscribeCommand)> {
    let series_id = OptionSeriesId::new_derived(
        Venue::new("DERIBIT"),
        Ustr::from("BTC"),
        Ustr::from("BTC"),
        UnixNanos::from(1_704_067_200_000_000_000u64),
    );

    vec![
        (
            "instrument status",
            SubscribeCommand::InstrumentStatus(SubscribeInstrumentStatus::new(
                instrument_id,
                Some(client_id),
                Some(venue),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            )),
        ),
        (
            "instrument close",
            SubscribeCommand::InstrumentClose(SubscribeInstrumentClose::new(
                instrument_id,
                Some(client_id),
                Some(venue),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            )),
        ),
        (
            "option chain",
            SubscribeCommand::OptionChain(SubscribeOptionChain::new(
                series_id,
                StrikeRange::Fixed(vec![Price::from("50000")]),
                Some(1_000),
                UUID4::new(),
                UnixNanos::default(),
                Some(client_id),
                Some(venue),
                None,
            )),
        ),
    ]
}

fn assert_no_streaming_types(msgbus: &MessageBus, case_name: &str) {
    for payload_type in data_streaming_payload_types() {
        assert!(
            !msgbus.is_streaming_type(payload_type),
            "{case_name} should not register {}",
            payload_type.as_str()
        );
    }
}

fn assert_only_streaming_type(msgbus: &MessageBus, expected: BusPayloadType, case_name: &str) {
    for payload_type in data_streaming_payload_types() {
        assert_eq!(
            msgbus.is_streaming_type(payload_type),
            payload_type == expected,
            "{case_name} streaming registration mismatch for {}",
            payload_type.as_str()
        );
    }
}

fn data_streaming_payload_types() -> Vec<BusPayloadType> {
    vec![
        BusPayloadType::Custom(Ustr::from("RustTestCustomData")),
        BusPayloadType::Instrument,
        BusPayloadType::OrderBookDeltas,
        BusPayloadType::OrderBookDepth,
        BusPayloadType::QuoteTick,
        BusPayloadType::TradeTick,
        BusPayloadType::Bar,
        BusPayloadType::MarkPriceUpdate,
        BusPayloadType::IndexPriceUpdate,
        BusPayloadType::FundingRateUpdate,
        BusPayloadType::OptionGreeks,
    ]
}

fn book_deltas_subscribe_count(recorded: &[DataCommand], instrument_id: InstrumentId) -> usize {
    recorded
        .iter()
        .filter(|cmd| {
            matches!(
                cmd,
                DataCommand::Subscribe(SubscribeCommand::BookDeltas(cmd))
                    if cmd.single_instrument_id().unwrap() == instrument_id
            )
        })
        .count()
}

fn book_deltas_unsubscribe_count(recorded: &[DataCommand], instrument_id: InstrumentId) -> usize {
    recorded
        .iter()
        .filter(|cmd| {
            matches!(
                cmd,
                DataCommand::Unsubscribe(UnsubscribeCommand::BookDeltas(cmd))
                    if cmd.single_instrument_id().unwrap() == instrument_id
            )
        })
        .count()
}

fn execute_book_delta_subscribe(
    data_engine: &Rc<RefCell<DataEngine>>,
    instrument_id: InstrumentId,
    client_id: ClientId,
    venue: Venue,
) {
    execute_book_delta_subscribe_for_route(
        data_engine,
        instrument_id,
        Some(client_id),
        Some(venue),
    );
}

fn execute_book_delta_subscribe_for_route(
    data_engine: &Rc<RefCell<DataEngine>>,
    instrument_id: InstrumentId,
    client_id: Option<ClientId>,
    venue: Option<Venue>,
) {
    let subscribe = SubscribeBookDeltas::new(
        instrument_id,
        BookType::L2_MBP,
        client_id,
        venue,
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
            subscribe,
        )));
}

fn execute_book_delta_unsubscribe(
    data_engine: &Rc<RefCell<DataEngine>>,
    instrument_id: InstrumentId,
    client_id: ClientId,
    venue: Venue,
) {
    execute_book_delta_unsubscribe_for_route(
        data_engine,
        instrument_id,
        Some(client_id),
        Some(venue),
    );
}

fn execute_book_delta_unsubscribe_for_route(
    data_engine: &Rc<RefCell<DataEngine>>,
    instrument_id: InstrumentId,
    client_id: Option<ClientId>,
    venue: Option<Venue>,
) {
    let unsubscribe = UnsubscribeBookDeltas::new(
        instrument_id,
        client_id,
        venue,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    data_engine
        .borrow_mut()
        .execute(DataCommand::Unsubscribe(UnsubscribeCommand::BookDeltas(
            unsubscribe,
        )));
}

fn synthetic_instrument_id() -> InstrumentId {
    InstrumentId::new(Symbol::new("BTC-ETH-INDEX"), Venue::synthetic())
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

fn unsubscribe_synthetic_trades_cmd(instrument_ids: impl IntoInstrumentIds) -> DataCommand {
    DataCommand::Unsubscribe(UnsubscribeCommand::Trades(UnsubscribeTrades::new(
        instrument_ids,
        None,
        Some(Venue::synthetic()),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )))
}

fn build_synthetic_subscribe(
    variant: &str,
    instrument_id: InstrumentId,
    client_id: ClientId,
    venue: Venue,
    ts_init: UnixNanos,
) -> DataCommand {
    let id = UUID4::new();

    match variant {
        "instrument" => {
            DataCommand::Subscribe(SubscribeCommand::Instrument(SubscribeInstrument::new(
                instrument_id,
                Some(client_id),
                Some(venue),
                id,
                ts_init,
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
                ts_init,
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
                ts_init,
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
                ts_init,
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
    ts_init: UnixNanos,
) -> DataCommand {
    let id = UUID4::new();

    match variant {
        "instrument" => {
            DataCommand::Unsubscribe(UnsubscribeCommand::Instrument(UnsubscribeInstrument::new(
                instrument_id,
                Some(client_id),
                Some(venue),
                id,
                ts_init,
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
                ts_init,
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
                ts_init,
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
                ts_init,
                None,
                None,
            ),
        )),
        other => panic!("unknown synthetic unsubscribe variant: {other}"),
    }
}

#[rstest]
#[case((
    "Quotes",
    "quote identifier lists cannot mix client, synthetic, and spread routes"
))]
#[case(("Instrument", REJECTED_NATIVE_BATCH_ERROR))]
#[case(("InstrumentStatus", REJECTED_NATIVE_BATCH_ERROR))]
#[case(("InstrumentClose", REJECTED_NATIVE_BATCH_ERROR))]
#[case(("OptionGreeks", REJECTED_NATIVE_BATCH_ERROR))]
fn test_rejected_actor_list_release_preserves_peer(
    #[case] rejection: (&str, &str),
    #[values((false, false), (false, true), (true, false), (true, true))] release_order: (
        bool,
        bool,
    ),
    #[values(false, true)] invalid_first: bool,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let (family, expected_error) = rejection;
    let (partial, peer_first) = release_order;
    let mut actor = DataActorCore::new(Default::default());
    let actor_clock = Rc::clone(&clock);
    actor
        .register(TraderId::test_default(), actor_clock, Rc::clone(&cache))
        .unwrap();
    let engine = create_snapshot_test_engine(Rc::clone(&clock), Rc::clone(&cache));
    let recorder = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut engine.borrow_mut(),
    );
    let endpoint_engine = Rc::clone(&engine);
    let actor_commands = Rc::new(RefCell::new(Vec::new()));
    let commands = Rc::clone(&actor_commands);
    let handler = TypedIntoHandler::from(move |command: DataCommand| {
        commands.borrow_mut().push(command.clone());
        endpoint_engine.borrow_mut().execute(command);
    });

    msgbus::register_data_command_endpoint(
        MessagingSwitchboard::data_engine_queue_execute(),
        handler,
    );
    let real = InstrumentId::from("AUD/USD.SIM");
    let synthetic = synthetic_instrument_id();
    let fields = json!({
        "instrument_ids": [real], "client_id": client_id, "venue": venue,
        "command_id": UUID4::new(), "ts_init": 17, "correlation_id": UUID4::new(),
        "params": {"owner": 23, "start_ns": 29}
    });
    let peer: SubscribeCommand = serde_json::from_value(json!({family: fields})).unwrap();
    engine.borrow_mut().execute_subscribe(peer.clone()).unwrap();

    let ids = if invalid_first {
        [synthetic, real]
    } else {
        [real, synthetic]
    };

    let params = Some(params_from_json(json!({"owner": 31, "start_ns": 37})));

    let error = match family {
        "Quotes" => actor.subscribe_quotes(
            TypedHandler::from(|_: &QuoteTick| {}),
            ids,
            Some(client_id),
            params,
        ),
        "Instrument" => actor.subscribe_instrument(
            TypedHandler::from(|_: &InstrumentAny| {}),
            ids,
            Some(client_id),
            params,
        ),
        "InstrumentStatus" => actor.subscribe_instrument_status(
            ShareableMessageHandler::from_typed(|_: &InstrumentStatus| {}),
            ids,
            Some(client_id),
            params,
        ),
        "InstrumentClose" => actor.subscribe_instrument_close(
            ShareableMessageHandler::from_typed(|_: &InstrumentClose| {}),
            ids,
            Some(client_id),
            params,
        ),
        "OptionGreeks" => actor.subscribe_option_greeks(
            TypedHandler::from(|_: &OptionGreeks| {}),
            ids,
            Some(client_id),
            params,
        ),
        _ => unreachable!(),
    }
    .unwrap_err();

    assert_eq!(error.to_string(), expected_error);
    assert!(actor_commands.borrow().is_empty());
    let release = peer
        .clone()
        .into_unsubscribe(UUID4::new(), UnixNanos::from(41), None);

    if peer_first {
        engine.borrow_mut().execute_unsubscribe(&release).unwrap();
        assert_eq!(recorder.borrow().len(), 2);
    }

    let mut release_actor = |ids: Vec<InstrumentId>| match family {
        "Quotes" => actor.unsubscribe_quotes(ids, Some(client_id), None),
        "Instrument" => actor.unsubscribe_instrument(ids, Some(client_id), None),
        "InstrumentStatus" => actor.unsubscribe_instrument_status(ids, Some(client_id), None),
        "InstrumentClose" => actor.unsubscribe_instrument_close(ids, Some(client_id), None),
        "OptionGreeks" => actor.unsubscribe_option_greeks(ids, Some(client_id), None),
        _ => unreachable!(),
    };

    release_actor(if partial { vec![real] } else { ids.to_vec() }).unwrap();
    if partial {
        release_actor(vec![synthetic]).unwrap();
    }

    assert!(actor_commands.borrow().is_empty());

    if !peer_first {
        assert_eq!(recorder.borrow().len(), 1);
        engine.borrow_mut().execute_unsubscribe(&release).unwrap();
    }

    let commands = recorder.borrow();

    let [
        DataCommand::Subscribe(acquired),
        DataCommand::Unsubscribe(released),
    ] = commands.as_slice()
    else {
        panic!("expected only the peer's acquisition and release: {commands:?}");
    };

    assert_eq!(
        serde_json::to_value(acquired).unwrap(),
        serde_json::to_value(peer).unwrap()
    );
    assert_eq!(
        serde_json::to_value(released).unwrap(),
        serde_json::to_value(release).unwrap()
    );
}

const REJECTED_NATIVE_BATCH_ERROR: &str =
    "Cannot subscribe synthetic instruments for this data type";

#[rstest]
fn test_rejected_parent_does_not_retain_client_ownership(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut engine = data_engine.borrow_mut();
    let recorder = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(clock, cache, client_id, venue, None, &recorder, &mut engine);
    let first = InstrumentId::from("AUD/USD.SIM");
    let second = InstrumentId::from("GBP/USD.SIM");
    let peer = SubscribeCommand::Quotes(SubscribeQuotes::new(
        [first, second],
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::from(17),
        None,
        Some(params_from_json(json!({"owner": 23, "start_ns": 29}))),
    ));
    let rejected = SubscribeCommand::Quotes(SubscribeQuotes::new(
        [synthetic_instrument_id(), first, second],
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::from(31),
        None,
        Some(params_from_json(json!({"owner": 37, "start_ns": 41}))),
    ));
    engine.execute_subscribe(peer.clone()).unwrap();
    let error = engine.execute_subscribe(rejected.clone()).unwrap_err();
    let release = peer
        .clone()
        .into_unsubscribe(UUID4::new(), UnixNanos::from(43), None);
    engine.execute_unsubscribe(&release).unwrap();
    assert_eq!(recorder.borrow().len(), 2);
    let final_release = rejected.into_unsubscribe(UUID4::new(), UnixNanos::from(47), None);
    let release_error = engine.execute_unsubscribe(&final_release).unwrap_err();
    let commands = recorder.borrow();

    let [
        DataCommand::Subscribe(acquired),
        DataCommand::Unsubscribe(released),
    ] = commands.as_slice()
    else {
        panic!("expected one batched acquisition and release: {commands:?}");
    };

    assert_eq!(
        error.to_string(),
        "quote identifier lists cannot mix client, synthetic, and spread routes"
    );
    assert_eq!(release_error.to_string(), error.to_string());
    assert_eq!(
        serde_json::to_value(acquired).unwrap(),
        serde_json::to_value(peer).unwrap()
    );
    assert_eq!(
        serde_json::to_value(released).unwrap(),
        serde_json::to_value(release).unwrap()
    );
    assert_eq!(engine.subscribed_quotes(), Vec::<InstrumentId>::new());
}

#[rstest]
#[case("Instrument")]
#[case("BookDeltas")]
#[case("BookDepth")]
#[case("BookSnapshots")]
#[case("MarkPrices")]
#[case("IndexPrices")]
#[case("FundingRates")]
#[case("InstrumentStatus")]
#[case("InstrumentClose")]
#[case("OptionGreeks")]
fn test_external_instrument_lists_filter_overlap_and_keep_release_acquisitions(
    #[case] family: &str,
    _stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache = Rc::new(RefCell::new(Cache::default()));

    let mut engine = DataEngine::new(
        clock,
        cache,
        Some(DataEngineConfig {
            external_clients: Some(vec![client_id]),
            ..Default::default()
        }),
    );

    let first = InstrumentId::from("AUD/USD.SIM");
    let second = InstrumentId::from("EUR/USD.SIM");
    let third = InstrumentId::from("GBP/USD.SIM");
    let topic = format!("commands.data.{client_id}");
    let (handler, acquisitions) = get_any_saving_handler::<SubscribeCommand>(None);
    msgbus::subscribe_any(topic.as_str().into(), handler, None);
    let (handler, releases) = get_any_saving_handler::<UnsubscribeCommand>(None);
    msgbus::subscribe_any(topic.as_str().into(), handler, None);
    let mut fields = serde_json::json!({"instrument_ids": [first, second], "client_id": client_id,
        "venue": venue, "command_id": UUID4::new(), "ts_init": 41, "correlation_id": UUID4::new(), "params": {"owner": 59}});

    if family.starts_with("Book") {
        fields["book_type"] = serde_json::json!(BookType::L2_MBP);
        fields["depth"] = serde_json::json!(7);
        if family == "BookSnapshots" {
            fields["interval_ms"] = serde_json::json!(13);
        } else {
            fields["managed"] = serde_json::json!(true);
        }
    }

    let original: SubscribeCommand =
        serde_json::from_value(serde_json::json!({family: fields})).unwrap();
    fields["instrument_ids"] = serde_json::json!([second, third]);
    fields["command_id"] = serde_json::json!(UUID4::new());
    fields["params"] = serde_json::json!({"owner": 61});
    let overlapping: SubscribeCommand =
        serde_json::from_value(serde_json::json!({family: fields})).unwrap();
    engine.execute_subscribe(original.clone()).unwrap();
    engine.execute_subscribe(overlapping.clone()).unwrap();
    let first_release_id = UUID4::new();
    engine
        .execute_unsubscribe(&original.clone().into_unsubscribe(
            first_release_id,
            UnixNanos::from(43),
            None,
        ))
        .unwrap();
    let after_first_release = releases.get_messages();
    let final_release_id = UUID4::new();
    engine
        .execute_unsubscribe(&overlapping.clone().into_unsubscribe(
            final_release_id,
            UnixNanos::from(47),
            None,
        ))
        .unwrap();
    let expected_first = original
        .with_instrument_ids(first)
        .unwrap()
        .into_unsubscribe(first_release_id, UnixNanos::from(43), None);
    let expected_final = vec![
        expected_first.clone(),
        original
            .with_instrument_ids(second)
            .unwrap()
            .into_unsubscribe(final_release_id, UnixNanos::from(47), None),
        overlapping
            .with_instrument_ids(third)
            .unwrap()
            .into_unsubscribe(final_release_id, UnixNanos::from(47), None),
    ];
    assert_eq!(
        serde_json::to_value(acquisitions.get_messages()).unwrap(),
        serde_json::to_value([original, overlapping.with_instrument_ids(third).unwrap()]).unwrap()
    );
    assert_eq!(
        serde_json::to_value(after_first_release).unwrap(),
        serde_json::to_value([expected_first]).unwrap()
    );
    assert_eq!(
        serde_json::to_value(releases.get_messages()).unwrap(),
        serde_json::to_value(expected_final).unwrap()
    );
}

#[rstest]
#[case("Instrument")]
#[case("BookDeltas")]
#[case("BookDepth")]
#[case("BookSnapshots")]
#[case("InstrumentStatus")]
#[case("InstrumentClose")]
#[case("OptionGreeks")]
fn test_native_instrument_batch_rejects_synthetic_before_dispatch(
    #[case] family: &str,
    #[values(true, false)] subscribe: bool,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut engine = data_engine.borrow_mut();
    let recorder = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(clock, cache, client_id, venue, None, &recorder, &mut engine);
    let first = InstrumentId::from("AUD/USD.SIM");
    let mut fields = serde_json::json!({
        "instrument_ids": [first, synthetic_instrument_id()],
        "client_id": client_id, "venue": venue, "command_id": UUID4::new(),
        "ts_init": 41, "correlation_id": UUID4::new(), "params": {"owner": 59}
    });

    if family.starts_with("Book") {
        fields["book_type"] = serde_json::json!(BookType::L2_MBP);
        fields["depth"] = serde_json::json!(7);
        if family == "BookSnapshots" {
            fields["interval_ms"] = serde_json::json!(13);
        } else {
            fields["managed"] = serde_json::json!(true);
        }
    }

    let command: SubscribeCommand =
        serde_json::from_value(serde_json::json!({family: fields})).unwrap();

    let result = if subscribe {
        engine.execute_subscribe(command)
    } else {
        engine.execute_unsubscribe(&command.into_unsubscribe(
            UUID4::new(),
            UnixNanos::from(43),
            None,
        ))
    };

    let action = if subscribe {
        "subscribe"
    } else {
        "unsubscribe"
    };

    assert_eq!(
        result.unwrap_err().to_string(),
        format!("Cannot {action} synthetic instruments for this data type")
    );
    assert_eq!(recorder.borrow().len(), 0);
}

#[rstest]
#[case("BookDeltas")]
#[case("BookDepth")]
#[case("BookSnapshots")]
fn test_actor_mixed_venue_book_batch_releases_engine_ownership(
    #[case] family: &str,
    audusd_sim: CurrencyPair,
    mut gbpusd_sim: CurrencyPair,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut actor = DataActorCore::new(Default::default());
    let actor_clock = Rc::clone(&clock);
    actor
        .register(TraderId::test_default(), actor_clock, Rc::clone(&cache))
        .unwrap();
    let engine = create_snapshot_test_engine(Rc::clone(&clock), Rc::clone(&cache));
    let recorder = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        Rc::clone(&clock),
        Rc::clone(&cache),
        client_id,
        venue,
        None,
        &recorder,
        &mut engine.borrow_mut(),
    );
    let endpoint_engine = Rc::clone(&engine);
    let handler = TypedIntoHandler::from(move |command: DataCommand| {
        endpoint_engine.borrow_mut().execute(command);
    });

    msgbus::register_data_command_endpoint(
        MessagingSwitchboard::data_engine_queue_execute(),
        handler,
    );
    gbpusd_sim.id = InstrumentId::from("GBP/USD.OTHER");
    let ids = [audusd_sim.id, gbpusd_sim.id];
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(gbpusd_sim))
        .unwrap();
    let interval = NonZeroUsize::new(13).unwrap();
    let params: Params = serde_json::from_value(serde_json::json!({"route": 59})).unwrap();

    match family {
        "BookDeltas" => actor.subscribe_book_deltas(
            TypedHandler::from(|_: &OrderBookDeltas| {}),
            ids,
            BookType::L2_MBP,
            None,
            Some(client_id),
            true,
            Some(params),
        ),
        "BookDepth" => actor.subscribe_book_depth(
            TypedHandler::from(|_: &OrderBookDepth| {}),
            ids,
            BookType::L2_MBP,
            None,
            Some(client_id),
            true,
            Some(params),
        ),
        "BookSnapshots" => actor.subscribe_book_at_interval(
            TypedHandler::from(|_: &OrderBook| {}),
            ids,
            BookType::L2_MBP,
            None,
            interval,
            Some(client_id),
            Some(params),
        ),
        _ => unreachable!(),
    }
    .unwrap();

    let acquired = recorder.borrow().len();
    let timers_acquired = clock.borrow().timer_count();

    match family {
        "BookDeltas" => actor.unsubscribe_book_deltas(ids, Some(client_id), None),
        "BookDepth" => actor.unsubscribe_book_depth(ids, Some(client_id), None),
        "BookSnapshots" => actor.unsubscribe_book_at_interval(ids, interval, Some(client_id), None),
        _ => unreachable!(),
    }
    .unwrap();

    let calls = if family == "BookSnapshots" { 2 } else { 1 };
    assert_eq!(acquired, calls);
    assert_eq!(timers_acquired, usize::from(family == "BookSnapshots"));
    assert_eq!(recorder.borrow().len(), calls * 2);
    assert_eq!(
        engine.borrow().subscribed_book_deltas(),
        Vec::<InstrumentId>::new()
    );
    assert_eq!(
        engine.borrow().subscribed_book_depth(),
        Vec::<InstrumentId>::new()
    );
    assert_eq!(
        engine.borrow().subscribed_book_snapshots(),
        Vec::<InstrumentId>::new()
    );
    assert_eq!(clock.borrow().timer_count(), 0);

    for id in ids {
        assert_eq!(
            msgbus::subscriber_count_deltas(switchboard::get_book_deltas_topic(id)),
            0
        );
        assert_eq!(
            msgbus::subscriber_count_depth(switchboard::get_book_depth_topic(id)),
            0
        );
    }
}

#[rstest]
#[case::native(false, true)]
#[case::external_overlap(true, true)]
#[case::external_grouped_release(true, false)]
fn test_bar_lists_filter_overlap_and_retain_release_acquisitions(
    #[case] external: bool,
    #[case] overlap: bool,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;

    let config = DataEngineConfig {
        external_clients: external.then(|| vec![client_id]),
        ..DataEngineConfig::default()
    };

    let mut engine = DataEngine::new(
        Rc::clone(&clock) as Rc<RefCell<dyn Clock>>,
        Rc::clone(&cache),
        Some(config),
    );
    let recorder = Rc::new(RefCell::new(Vec::new()));
    if !external {
        register_mock_client(clock, cache, client_id, venue, None, &recorder, &mut engine);
    }

    let topic = format!("commands.data.{client_id}");
    let (handler, subscribes) = get_any_saving_handler::<SubscribeCommand>(None);
    msgbus::subscribe_any(topic.as_str().into(), handler, None);
    let (handler, releases) = get_any_saving_handler::<UnsubscribeCommand>(None);
    msgbus::subscribe_any(topic.as_str().into(), handler, None);
    let first = BarType::from("AUD/USD.SIM-1-MINUTE-LAST-EXTERNAL");
    let second = BarType::from("AUD/USD.SIM-5-MINUTE-LAST-EXTERNAL");
    let third = BarType::from("GBP/USD.SIM-1-MINUTE-BID-EXTERNAL");

    let first_command = SubscribeBars::new(
        [second, first, second],
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::from(17),
        Some(UUID4::new()),
        Some(params_from_json(json!({"route": 37, "start_ns": 91}))),
    );

    let second_command = SubscribeBars::new(
        if overlap {
            vec![first, third]
        } else {
            vec![third]
        },
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::from(23),
        Some(UUID4::new()),
        Some(params_from_json(json!({"route": 59, "start_ns": 97}))),
    );

    for command in [&first_command, &first_command, &second_command] {
        engine
            .execute_subscribe(SubscribeCommand::Bars(command.clone()))
            .unwrap();
    }

    let release = UnsubscribeBars::new(
        [second, first, third],
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::from(31),
        Some(UUID4::new()),
        Some(params_from_json(json!({"route": "fallback"}))),
    );
    engine
        .execute_unsubscribe(&UnsubscribeCommand::Bars(release.clone()))
        .unwrap();

    if !external {
        assert_eq!(engine.subscribed_bars(), vec![first]);
    }

    let final_release = UnsubscribeBars {
        command_id: UUID4::new(),
        ..release.with_bar_types(first)
    };

    if overlap {
        engine
            .execute_unsubscribe(&UnsubscribeCommand::Bars(final_release.clone()))
            .unwrap();
    }

    let expected_subscribes = vec![first_command.clone(), second_command.with_bar_types(third)];

    let mut expected_releases = vec![
        (&first_command, second, &release),
        (&second_command, third, &release),
        (&first_command, first, &final_release),
    ]
    .into_iter()
    .map(|(retained, bar_type, release)| {
        SubscribeCommand::Bars(retained.with_bar_types(bar_type)).into_unsubscribe(
            release.command_id,
            release.ts_init,
            release.correlation_id,
        )
    })
    .collect::<Vec<_>>();

    if !overlap {
        expected_releases =
            vec![
                SubscribeCommand::Bars(first_command.with_bar_types([second, first]))
                    .into_unsubscribe(release.command_id, release.ts_init, release.correlation_id),
                SubscribeCommand::Bars(second_command.with_bar_types(third)).into_unsubscribe(
                    release.command_id,
                    release.ts_init,
                    release.correlation_id,
                ),
            ];
    }

    if external {
        assert_eq!(
            serde_json::to_value(subscribes.get_messages()).unwrap(),
            serde_json::to_value(
                expected_subscribes
                    .into_iter()
                    .map(SubscribeCommand::Bars)
                    .collect::<Vec<_>>()
            )
            .unwrap()
        );
        assert_eq!(
            serde_json::to_value(releases.get_messages()).unwrap(),
            serde_json::to_value(expected_releases).unwrap()
        );
    } else {
        let expected = expected_subscribes
            .into_iter()
            .map(|cmd| serde_json::to_value(SubscribeCommand::Bars(cmd)).unwrap())
            .chain(
                expected_releases
                    .into_iter()
                    .map(|cmd| serde_json::to_value(cmd).unwrap()),
            )
            .collect::<Vec<_>>();

        let actual = recorder
            .borrow()
            .iter()
            .map(|cmd| match cmd {
                DataCommand::Subscribe(cmd) => serde_json::to_value(cmd).unwrap(),
                DataCommand::Unsubscribe(cmd) => serde_json::to_value(cmd).unwrap(),
                _ => panic!("unexpected command"),
            })
            .collect::<Vec<_>>();

        assert_eq!(actual, expected);
        assert_eq!(engine.subscribed_bars(), Vec::<BarType>::new());
    }
}

#[rstest]
#[case::empty(vec![], "bar types cannot be empty")]
#[case::duplicate(vec![BarType::from("AUD/USD.SIM-1-MINUTE-LAST-EXTERNAL"); 2], "bar types must be unique")]
#[case::mixed_venues(vec![BarType::from("AUD/USD.SIM-1-MINUTE-LAST-EXTERNAL"), BarType::from("AUD/USD.OTHER-5-MINUTE-LAST-INTERNAL")], "multiple bar type venues require an explicit client ID")]
fn test_engine_bar_lists_reject_invalid_intents_before_effects(
    #[case] bar_types: Vec<BarType>,
    #[case] error: &str,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut engine = data_engine.borrow_mut();
    let recorder = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(clock, cache, client_id, venue, None, &recorder, &mut engine);

    let command = SubscribeBars {
        bar_types,
        client_id: None,
        venue: Some(venue),
        command_id: UUID4::new(),
        ts_init: UnixNanos::from(41),
        correlation_id: None,
        params: None,
    };

    let release = UnsubscribeBars {
        bar_types: command.bar_types.clone(),
        client_id: None,
        venue: Some(venue),
        command_id: UUID4::new(),
        ts_init: UnixNanos::from(43),
        correlation_id: None,
        params: None,
    };

    assert_eq!(
        engine
            .execute_subscribe(SubscribeCommand::Bars(command))
            .unwrap_err()
            .to_string(),
        error
    );
    assert_eq!(
        engine
            .execute_unsubscribe(&UnsubscribeCommand::Bars(release))
            .unwrap_err()
            .to_string(),
        error
    );
    assert_eq!(engine.subscribed_bars(), Vec::<BarType>::new());
    assert_eq!(engine.subscribed_trades(), Vec::<InstrumentId>::new());
    assert!(recorder.borrow().is_empty());
}

#[rstest]
#[case("Instrument")]
#[case("BookDeltas")]
#[case("BookDepth")]
#[case("Quotes")]
#[case("Trades")]
#[case("MarkPrices")]
#[case("IndexPrices")]
#[case("FundingRates")]
#[case("InstrumentStatus")]
#[case("InstrumentClose")]
#[case("OptionGreeks")]
#[case("Bars")]
fn test_direct_lists_reach_hooks_with_parent_metadata(
    #[case] family: &str,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut engine = data_engine.borrow_mut();
    let recorder = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(clock, cache, client_id, venue, None, &recorder, &mut engine);
    let mut fields = json!({
        "instrument_ids": ["GBP/USD.SIM", "AUD/USD.SIM", "GBP/USD.SIM"],
        "client_id": client_id, "venue": venue, "command_id": UUID4::new(),
        "ts_init": 43, "correlation_id": UUID4::new(),
        "params": {"route": 59, "start_ns": 71, "nested": {"value": 83}}
    });

    if family == "Bars" {
        fields.as_object_mut().unwrap().remove("instrument_ids");
        fields["bar_types"] = json!([
            "AUD/USD.SIM-5-MINUTE-LAST-EXTERNAL",
            "AUD/USD.SIM-1-MINUTE-BID-EXTERNAL",
            "AUD/USD.SIM-5-MINUTE-LAST-EXTERNAL"
        ]);
    } else if family.starts_with("Book") {
        fields["book_type"] = json!(BookType::L2_MBP);
        fields["depth"] = json!(7);
        fields["managed"] = json!(false);
    }

    let command: SubscribeCommand = serde_json::from_value(json!({family: fields})).unwrap();
    engine.execute_subscribe(command.clone()).unwrap();
    let release = command.clone().into_unsubscribe(
        UUID4::new(),
        UnixNanos::from(97),
        Some(command.command_id()),
    );
    engine.execute_unsubscribe(&release).unwrap();

    let actual: Vec<_> = recorder
        .borrow()
        .iter()
        .map(|command| match command {
            DataCommand::Subscribe(command) => serde_json::to_value(command).unwrap(),
            DataCommand::Unsubscribe(command) => serde_json::to_value(command).unwrap(),
            _ => panic!("unexpected command"),
        })
        .collect();

    assert_eq!(
        actual,
        vec![
            serde_json::to_value(command).unwrap(),
            serde_json::to_value(release).unwrap()
        ]
    );
}

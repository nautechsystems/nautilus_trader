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

use super::*;

#[rstest]
fn test_unsubscribe_depth_keeps_deltas_book_updater(
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

    // Subscribe to both deltas and depth
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

    let sub_depth = DataCommand::Subscribe(SubscribeCommand::BookDepth(SubscribeBookDepth::new(
        audusd_sim.id,
        BookType::L2_MBP,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        false,
        None,
        None,
    )));
    data_engine.execute(sub_depth);

    // Only managed deltas own a book updater
    assert_eq!(msgbus::subscriber_count_deltas(deltas_topic), 1);
    assert_eq!(msgbus::subscriber_count_depth(depth_topic), 0);

    // Unsubscribe from depth only
    let unsub_depth =
        DataCommand::Unsubscribe(UnsubscribeCommand::BookDepth(UnsubscribeBookDepth::new(
            audusd_sim.id,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        )));
    data_engine.execute(unsub_depth);

    // BookUpdater should remain subscribed to deltas but not depth
    assert_eq!(msgbus::subscriber_count_deltas(deltas_topic), 1);
    assert_eq!(msgbus::subscriber_count_depth(depth_topic), 0);

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
    assert_eq!(msgbus::subscriber_count_depth(depth_topic), 0);
}

#[rstest]
fn test_book_depth_releases_after_final_route_owner(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );
    let depth_topic = switchboard::get_book_depth_topic(audusd_sim.id);

    for _ in 0..2 {
        data_engine.execute(DataCommand::Subscribe(SubscribeCommand::BookDepth(
            SubscribeBookDepth::new(
                audusd_sim.id,
                BookType::L2_MBP,
                Some(client_id),
                Some(venue),
                UUID4::new(),
                UnixNanos::default(),
                NonZeroUsize::new(10),
                true,
                None,
                None,
            ),
        )));
    }

    assert_eq!(recorder.borrow().len(), 1);
    assert_eq!(msgbus::subscriber_count_depth(depth_topic), 1);

    let unsubscribe = || {
        DataCommand::Unsubscribe(UnsubscribeCommand::BookDepth(UnsubscribeBookDepth::new(
            audusd_sim.id,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        )))
    };

    data_engine.execute(unsubscribe());
    assert_eq!(recorder.borrow().len(), 1);
    assert_eq!(msgbus::subscriber_count_depth(depth_topic), 1);

    data_engine.execute(unsubscribe());
    assert_eq!(recorder.borrow().len(), 2);
    assert!(matches!(
        &recorder.borrow()[1],
        DataCommand::Unsubscribe(UnsubscribeCommand::BookDepth(_))
    ));
    assert_eq!(msgbus::subscriber_count_depth(depth_topic), 0);
}

#[rstest]
fn test_emit_quotes_from_book_depths_publishes_top_of_book(stub_msgbus: Rc<RefCell<MessageBus>>) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
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

    data_engine.process_data(Data::BookDepth(Box::new(depth.clone())));

    let messages = saver.get_messages();
    assert_eq!(
        messages.len(),
        1,
        "depth should emit exactly one synthetic quote",
    );
    let cached_quote = cache.borrow().quote(&instrument_id).copied();
    assert!(cached_quote.is_some(), "synthetic quote should be cached",);

    // Same top-of-book: must not republish
    data_engine.process_data(Data::BookDepth(Box::new(depth.clone())));
    assert_eq!(saver.get_messages().len(), 1);

    // Shifted top-of-book: must republish
    let mut shifted = depth.clone();

    shifted.bids[0] = BookOrder::new(
        depth.bids[0].side,
        Price::new(98.50, 2),
        depth.bids[0].size,
        depth.bids[0].order_id,
    );
    shifted.ts_event = UnixNanos::from(depth.ts_event.as_u64() + 1);
    shifted.ts_init = UnixNanos::from(depth.ts_init.as_u64() + 1);
    data_engine.process_data(Data::BookDepth(Box::new(shifted)));

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
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let config = DataEngineConfig {
        emit_quotes_from_book_depths: true,
        ..DataEngineConfig::default()
    };

    let mut data_engine = DataEngine::new(clock, cache.clone(), Some(config));

    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let padded_bids: [BookOrder; DEPTH10_LEN] = [BookOrder::default(); DEPTH10_LEN];
    let padded_asks: [BookOrder; DEPTH10_LEN] = [BookOrder::default(); DEPTH10_LEN];

    let depth = OrderBookDepth::new(
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

    data_engine.process_data(Data::BookDepth(Box::new(depth)));

    assert!(
        saver.get_messages().is_empty(),
        "fully padded no-side depth must not publish a synthetic quote",
    );
    assert!(
        cache.borrow().quote(&instrument_id).is_none(),
        "no quote should be cached for invalid depth padding",
    );
}

#[rstest]
fn test_composite_book_deltas_route_to_per_underlying_book(
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

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);

    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
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
    data_engine.process_data(Data::BookDelta(delta));

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

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);

    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
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
        "ESZ1 book must reflect exactly its own delta",
    );
    assert_eq!(
        cache_view.order_book(&esh2_id).unwrap().update_count,
        1,
        "ESH2 book must reflect exactly its own delta",
    );
}

#[rstest]
fn test_composite_and_exact_book_deltas_apply_once_per_publish(
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

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);

    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
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

    data_engine.process_data(Data::BookDelta(
        OrderBookDeltaTestBuilder::new(esz1_id).build(),
    ));

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
fn test_snapshot_after_deltas_keeps_delta_handler_alive(
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

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
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
    data_engine.process_data(Data::BookDepth(Box::new(depth)));

    assert_eq!(cache.borrow().order_book(&esz1_id).unwrap().update_count, 0);
    data_engine.process_data(Data::BookDelta(
        OrderBookDeltaTestBuilder::new(esz1_id).build(),
    ));

    let cache_view = cache.borrow();
    let book = cache_view.order_book(&esz1_id).unwrap();
    assert_eq!(book.update_count, 1);
    assert_eq!(
        msgbus::subscriber_count_deltas(switchboard::get_book_deltas_topic(esz1_id)),
        1
    );
    assert_eq!(
        msgbus::subscriber_count_depth(switchboard::get_book_depth_topic(esz1_id)),
        0
    );
}

#[rstest]
fn test_parent_book_deltas_filters_by_instrument_class(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
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
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
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
    let clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
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
fn test_depth_parent_subscribe_with_unparsable_id_returns_error(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let venue = Venue::new("BETFAIR");

    let mut data_engine = DataEngine::new(clock, cache.clone(), None);
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
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
    let sub = DataCommand::Subscribe(SubscribeCommand::BookDepth(SubscribeBookDepth::new(
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
            "parent depth subscribe with an unparsable Betfair runner id must NOT create a book",
        );
    }

    assert!(
        !data_engine.subscribed_book_depth().contains(&runner),
        "rejected parent depth subscribe must NOT leave the id in book_depth_subs",
    );

    // Retrying without the parent flag on the same id must succeed.
    let retry = DataCommand::Subscribe(SubscribeCommand::BookDepth(SubscribeBookDepth::new(
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
        "concrete depth subscribe after a rejected parent attempt must still create the exact-id book",
    );
}

#[rstest]
fn test_emit_quotes_from_book_publishes_on_delta_apply(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let config = DataEngineConfig {
        emit_quotes_from_book: true,
        ..DataEngineConfig::default()
    };

    let mut data_engine = DataEngine::new(clock, cache.clone(), Some(config));

    let deltas = stub_deltas();
    let instrument_id = deltas.instrument_id;
    let venue = instrument_id.venue;

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

    let deltas = Box::new(deltas);
    data_engine.process_data(Data::BookDeltas(deltas.clone()));

    assert_eq!(
        saver.get_messages().len(),
        1,
        "managed BookDeltas with emit_quotes_from_book must publish a top-of-book quote",
    );

    // Same deltas, same top-of-book: idempotent
    data_engine.process_data(Data::BookDeltas(deltas));
    assert_eq!(saver.get_messages().len(), 1);
}

#[rstest]
fn test_emit_quotes_from_book_publishes_on_depth_apply(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let config = DataEngineConfig {
        emit_quotes_from_book: true,
        ..DataEngineConfig::default()
    };

    let mut data_engine = DataEngine::new(clock.clone(), cache.clone(), Some(config));

    let depth = stub_depth10();
    let instrument_id = depth.instrument_id;
    let venue = instrument_id.venue;

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

    let sub = DataCommand::Subscribe(SubscribeCommand::BookDepth(SubscribeBookDepth::new(
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

    data_engine.process_data(Data::BookDepth(Box::new(depth)));

    let messages = saver.get_messages();
    assert_eq!(
        messages.len(),
        1,
        "managed depth subscription with emit_quotes_from_book must publish a top-of-book quote",
    );
}

#[rstest]
#[case::deltas(BookSubscriptionKind::Deltas)]
#[case::depth(BookSubscriptionKind::Depth)]
fn test_shared_book_subscription_retries_after_client_failure(
    #[case] kind: BookSubscriptionKind,
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder = Rc::new(RefCell::new(Vec::new()));
    register_failing_subscribe_client(
        clock,
        cache,
        client_id,
        venue,
        &recorder,
        kind.failure(),
        &mut data_engine,
    );

    let subscribe = |command_id| match kind {
        BookSubscriptionKind::Deltas => SubscribeCommand::BookDeltas(SubscribeBookDeltas::new(
            audusd_sim.id,
            BookType::L3_MBO,
            Some(client_id),
            Some(venue),
            command_id,
            UnixNanos::from(1),
            NonZeroUsize::new(5),
            true,
            None,
            None,
        )),
        BookSubscriptionKind::Depth => SubscribeCommand::BookDepth(SubscribeBookDepth::new(
            audusd_sim.id,
            BookType::L2_MBP,
            Some(client_id),
            Some(venue),
            command_id,
            UnixNanos::from(1),
            NonZeroUsize::new(5),
            true,
            None,
            None,
        )),
    };

    let first = subscribe(UUID4::new());
    let second = subscribe(UUID4::new());

    data_engine.execute(DataCommand::Subscribe(first));
    assert!(recorder.borrow().is_empty());

    data_engine.execute(DataCommand::Subscribe(second.clone()));

    let expected_subscribe = DataCommand::Subscribe(second);
    assert_eq!(
        recorder.borrow().as_slice(),
        std::slice::from_ref(&expected_subscribe)
    );

    let unsubscribe = |command_id| match kind {
        BookSubscriptionKind::Deltas => UnsubscribeCommand::BookDeltas(UnsubscribeBookDeltas::new(
            audusd_sim.id,
            Some(client_id),
            Some(venue),
            command_id,
            UnixNanos::from(2),
            None,
            None,
        )),
        BookSubscriptionKind::Depth => UnsubscribeCommand::BookDepth(UnsubscribeBookDepth::new(
            audusd_sim.id,
            Some(client_id),
            Some(venue),
            command_id,
            UnixNanos::from(2),
            None,
            None,
        )),
    };

    data_engine.execute(DataCommand::Unsubscribe(unsubscribe(UUID4::new())));
    assert_eq!(
        recorder.borrow().as_slice(),
        std::slice::from_ref(&expected_subscribe)
    );

    let expected_unsubscribe = DataCommand::Unsubscribe(unsubscribe(UUID4::new()));
    data_engine.execute(expected_unsubscribe.clone());
    assert_eq!(
        recorder.borrow().as_slice(),
        &[expected_subscribe, expected_unsubscribe]
    );
}

#[rstest]
fn test_shared_book_snapshot_retries_source_after_client_failure(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder = Rc::new(RefCell::new(Vec::new()));
    register_failing_subscribe_client(
        clock,
        cache,
        client_id,
        venue,
        &recorder,
        MockSubscribeFailure::BookDeltas,
        &mut data_engine,
    );
    let first_command_id = UUID4::new();

    let subscribe = |command_id, ts_init| {
        SubscribeCommand::BookSnapshots(SubscribeBookSnapshots::new(
            audusd_sim.id,
            BookType::L3_MBO,
            Some(client_id),
            Some(venue),
            command_id,
            ts_init,
            NonZeroUsize::new(5),
            NonZeroUsize::new(1000).unwrap(),
            None,
            None,
        ))
    };

    data_engine.execute(DataCommand::Subscribe(subscribe(
        first_command_id,
        UnixNanos::from(1),
    )));
    assert!(recorder.borrow().is_empty());

    data_engine.execute(DataCommand::Subscribe(subscribe(
        UUID4::new(),
        UnixNanos::from(2),
    )));

    let recorded = recorder.borrow();
    assert_eq!(recorded.len(), 1);

    let DataCommand::Subscribe(SubscribeCommand::BookDeltas(command)) = &recorded[0] else {
        panic!("expected a book deltas source subscription");
    };

    assert_eq!(command.instrument_id, audusd_sim.id);
    assert_eq!(command.book_type, BookType::L3_MBO);
    assert_eq!(command.client_id, Some(client_id));
    assert_eq!(command.venue, Some(venue));
    assert_eq!(command.ts_init, UnixNanos::from(1));
    assert_eq!(command.depth, NonZeroUsize::new(5));
    assert!(command.managed);
    assert_eq!(command.correlation_id, Some(first_command_id));
    assert_eq!(command.params, None);
    drop(recorded);

    let unsubscribe = || {
        DataCommand::Unsubscribe(UnsubscribeCommand::BookSnapshots(
            UnsubscribeBookSnapshots::new(
                audusd_sim.id,
                NonZeroUsize::new(1000).unwrap(),
                Some(client_id),
                Some(venue),
                UUID4::new(),
                UnixNanos::from(3),
                None,
                None,
            ),
        ))
    };

    data_engine.execute(unsubscribe());
    assert_eq!(recorder.borrow().len(), 1);
    data_engine.execute(unsubscribe());

    let recorded = recorder.borrow();
    assert_eq!(recorded.len(), 2);

    let DataCommand::Unsubscribe(UnsubscribeCommand::BookDeltas(command)) = &recorded[1] else {
        panic!("expected a book deltas source unsubscription");
    };

    assert_eq!(command.instrument_id, audusd_sim.id);
    assert_eq!(command.client_id, Some(client_id));
    assert_eq!(command.venue, Some(venue));
    assert_eq!(command.ts_init, UnixNanos::from(3));
    assert_eq!(command.correlation_id, Some(first_command_id));
    assert_eq!(command.params, None);
}

#[rstest]
fn test_book_snapshot_retains_existing_deltas_source(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();
    let recorder = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        client_id,
        venue,
        None,
        &recorder,
        &mut data_engine,
    );
    let snapshot_command_id = UUID4::new();

    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::BookDeltas(
        SubscribeBookDeltas::new(
            audusd_sim.id,
            BookType::L3_MBO,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::from(1),
            None,
            true,
            None,
            None,
        ),
    )));
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::BookSnapshots(
        SubscribeBookSnapshots::new(
            audusd_sim.id,
            BookType::L3_MBO,
            Some(client_id),
            Some(venue),
            snapshot_command_id,
            UnixNanos::from(2),
            None,
            NonZeroUsize::new(1000).unwrap(),
            None,
            None,
        ),
    )));
    assert_eq!(recorder.borrow().len(), 1);

    data_engine.execute(DataCommand::Unsubscribe(UnsubscribeCommand::BookDeltas(
        UnsubscribeBookDeltas::new(
            audusd_sim.id,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::from(3),
            None,
            None,
        ),
    )));
    assert_eq!(recorder.borrow().len(), 1);

    data_engine.execute(DataCommand::Unsubscribe(UnsubscribeCommand::BookSnapshots(
        UnsubscribeBookSnapshots::new(
            audusd_sim.id,
            NonZeroUsize::new(1000).unwrap(),
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::from(4),
            None,
            None,
        ),
    )));

    let recorded = recorder.borrow();
    assert_eq!(recorded.len(), 2);

    let DataCommand::Unsubscribe(UnsubscribeCommand::BookDeltas(command)) = &recorded[1] else {
        panic!("expected a book deltas source unsubscription");
    };

    assert_eq!(command.instrument_id, audusd_sim.id);
    assert_eq!(command.client_id, Some(client_id));
    assert_eq!(command.venue, Some(venue));
    assert_eq!(command.ts_init, UnixNanos::from(4));
    assert_eq!(command.correlation_id, Some(snapshot_command_id));
    assert_eq!(command.params, None);
}

#[rstest]
fn test_reset_clears_book_state_and_timers(
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
    assert_eq!(recorder.borrow().len(), 1);
    assert!(!data_engine.subscribed_book_snapshots().is_empty());
    assert!(!data_engine.clock().borrow().timer_names().is_empty());

    data_engine.reset();

    assert_eq!(msgbus::subscriber_count_deltas(deltas_topic), 0);
    assert_eq!(msgbus::subscriber_count_depth(depth_topic), 0);
    assert!(data_engine.subscribed_book_snapshots().is_empty());
    assert!(data_engine.clock().borrow().timer_names().is_empty());
    assert_eq!(data_engine.command_count(), 0);
    assert_eq!(data_engine.data_count(), 0);

    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::BookDeltas(
        SubscribeBookDeltas::new(
            audusd_sim.id,
            BookType::L3_MBO,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::from(2),
            None,
            true,
            None,
            None,
        ),
    )));

    assert_eq!(recorder.borrow().len(), 2);
}

#[rstest]
#[case::owned(false)]
#[case::borrowed(true)]
fn test_process_book_delta(
    #[case] borrowed: bool,
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
    dispatch_data(&mut data_engine, Data::BookDelta(delta), borrowed);
    let _cache = &data_engine.cache().borrow();
    let messages = saver.get_messages();

    assert_eq!(messages.len(), 1);
}

#[rstest]
fn test_process_book_delta_buffers_until_f_last(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let config = DataEngineConfig {
        buffer_deltas: true,
        ..DataEngineConfig::default()
    };

    let mut data_engine = DataEngine::new(clock, cache, Some(config));

    let (handler, saver) = get_typed_message_saving_handler::<OrderBookDeltas>(None);
    let topic = switchboard::get_book_deltas_topic(instrument_id);
    msgbus::subscribe_book_deltas(topic.into(), handler, None);

    let f_last = RecordFlag::F_LAST as u8;
    data_engine.process_data(Data::BookDelta(delta_with_flag(instrument_id, 1_000, 0)));
    data_engine.process_data(Data::BookDelta(delta_with_flag(instrument_id, 2_000, 0)));
    assert!(
        saver.get_messages().is_empty(),
        "buffered deltas must not publish before F_LAST"
    );

    data_engine.process_data(Data::BookDelta(delta_with_flag(
        instrument_id,
        3_000,
        f_last,
    )));
    let first = saver.get_messages();
    assert_eq!(first.len(), 1);
    assert_eq!(
        first[0]
            .deltas
            .iter()
            .map(|delta| delta.ts_event.as_u64())
            .collect::<Vec<_>>(),
        vec![1_000, 2_000, 3_000],
    );
    assert_eq!(first[0].flags, f_last);

    data_engine.process_data(Data::BookDelta(delta_with_flag(
        instrument_id,
        4_000,
        f_last,
    )));
    let second = saver.get_messages();
    assert_eq!(second.len(), 2);
    assert_eq!(second[1].deltas.len(), 1);
    assert_eq!(second[1].deltas[0].ts_event.as_u64(), 4_000);
}

#[rstest]
#[case::owned(false)]
#[case::borrowed(true)]
fn test_process_book_deltas_buffers_until_f_last(
    #[case] borrowed: bool,
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let config = DataEngineConfig {
        buffer_deltas: true,
        ..DataEngineConfig::default()
    };

    let mut data_engine = DataEngine::new(clock, cache, Some(config));

    let (handler, saver) = get_typed_message_saving_handler::<OrderBookDeltas>(None);
    let topic = switchboard::get_book_deltas_topic(instrument_id);
    msgbus::subscribe_book_deltas(topic.into(), handler, None);

    let f_last = RecordFlag::F_LAST as u8;

    let batch = OrderBookDeltas::new(
        instrument_id,
        vec![
            delta_with_flag(instrument_id, 1_000, 0),
            delta_with_flag(instrument_id, 2_000, f_last),
            delta_with_flag(instrument_id, 3_000, 0),
            delta_with_flag(instrument_id, 4_000, f_last),
        ],
    );
    dispatch_data(
        &mut data_engine,
        Data::BookDeltas(Box::new(batch)),
        borrowed,
    );

    let published = saver.get_messages();
    assert_eq!(published.len(), 2);
    assert_eq!(
        published[0]
            .deltas
            .iter()
            .map(|delta| delta.ts_event.as_u64())
            .collect::<Vec<_>>(),
        vec![1_000, 2_000],
    );
    assert_eq!(
        published[1]
            .deltas
            .iter()
            .map(|delta| delta.ts_event.as_u64())
            .collect::<Vec<_>>(),
        vec![3_000, 4_000],
    );
}

#[rstest]
#[case::owned(false)]
#[case::borrowed(true)]
fn test_process_book_deltas(
    #[case] borrowed: bool,
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

    let deltas = Box::new(stub_deltas());
    let (handler, saver) = get_typed_message_saving_handler::<OrderBookDeltas>(None);
    let topic = switchboard::get_book_deltas_topic(deltas.instrument_id);
    msgbus::subscribe_book_deltas(topic.into(), handler, None);

    let mut data_engine = data_engine.borrow_mut();
    dispatch_data(&mut data_engine, Data::BookDeltas(deltas.clone()), borrowed);
    let _cache = &data_engine.cache().borrow();
    let messages = saver.get_messages();

    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&deltas));
}

#[rstest]
#[case::owned(false)]
#[case::borrowed(true)]
fn test_process_book_depth(
    #[case] borrowed: bool,
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
    let client_id = data_client.client_id;
    let venue = data_client.venue;
    data_engine.borrow_mut().register_client(data_client, None);

    let sub = SubscribeBookDepth::new(
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
    let cmd = DataCommand::Subscribe(SubscribeCommand::BookDepth(sub));

    data_engine.borrow_mut().execute(cmd);

    let depth = stub_depth10();
    let (handler, saver) = get_typed_message_saving_handler::<OrderBookDepth>(None);
    let topic = switchboard::get_book_depth_topic(depth.instrument_id);
    msgbus::subscribe_book_depth(topic.into(), handler, None);

    let mut data_engine = data_engine.borrow_mut();
    dispatch_data(&mut data_engine, Data::from(depth.clone()), borrowed);
    let _cache = &data_engine.cache().borrow();
    let messages = saver.get_messages();

    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&depth));
}

#[rstest]
fn test_process_book_snapshot_publish(
    audusd_sim: CurrencyPair,
    clock: Rc<RefCell<VirtualClock>>,
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
    let deltas = Box::new(OrderBookDeltas::new(audusd_sim.id, vec![delta]));
    data_engine
        .borrow_mut()
        .process_data(Data::BookDeltas(deltas));

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
    clock: Rc<RefCell<VirtualClock>>,
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
fn test_process_book_snapshot_publish_for_multiple_intervals_same_instrument(
    audusd_sim: CurrencyPair,
    clock: Rc<RefCell<VirtualClock>>,
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
fn test_trim_to_bounds_trims_book_depth(audusd_sim: CurrencyPair) {
    let instrument_id = audusd_sim.id;
    let mut resp = DataResponse::BookDepth(BookDepthResponse::new(
        UUID4::new(),
        ClientId::test_default(),
        instrument_id,
        vec![
            book_depth_at(instrument_id, 1_000),
            book_depth_at(instrument_id, 2_000),
            book_depth_at(instrument_id, 3_000),
        ],
        Some(UnixNanos::from(1_500)),
        Some(UnixNanos::from(2_500)),
        UnixNanos::default(),
        None,
    ));

    resp.trim_to_bounds();

    let DataResponse::BookDepth(depths) = resp else {
        panic!("expected BookDepth variant");
    };

    let ts_inits: Vec<u64> = depths.data.iter().map(|d| d.ts_init.as_u64()).collect();
    assert_eq!(ts_inits, vec![2_000]);
}

#[rstest]
#[case::deltas_manage(false)]
#[case::depth_manages(true)]
fn test_managed_book_rejects_other_source_and_ignores_unmanaged_updates(
    managed_book_engine: Rc<RefCell<DataEngine>>,
    audusd_sim: CurrencyPair,
    client_id: ClientId,
    #[case] depth_source: bool,
) {
    let mut engine = managed_book_engine.borrow_mut();
    let owner = book_source_command(audusd_sim.id, client_id, depth_source, true);
    engine.execute_subscribe(owner).unwrap();
    let conflicting = book_source_command(audusd_sim.id, client_id, !depth_source, true);
    let error = engine.execute_subscribe(conflicting.clone()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("Conflicting managed book source")
    );
    engine
        .execute_unsubscribe(&conflicting.clone().into_unsubscribe(
            UUID4::new(),
            UnixNanos::from(103),
            Some(conflicting.command_id()),
        ))
        .unwrap();

    let unmanaged = book_source_command(audusd_sim.id, client_id, !depth_source, false);
    engine.execute_subscribe(unmanaged).unwrap();
    let (depth_handler, depths) = get_typed_message_saving_handler::<OrderBookDepth>(None);
    let (delta_handler, deltas) = get_typed_message_saving_handler::<OrderBookDeltas>(None);
    msgbus::subscribe_book_depth(
        switchboard::get_book_depth_topic(audusd_sim.id).into(),
        depth_handler,
        None,
    );
    msgbus::subscribe_book_deltas(
        switchboard::get_book_deltas_topic(audusd_sim.id).into(),
        delta_handler,
        None,
    );
    let mut full = stub_depth10();
    full.instrument_id = audusd_sim.id;
    full.sequence = 109;
    full.ts_event = UnixNanos::from(113);
    full.ts_init = UnixNanos::from(127);
    let mut expected = OrderBook::new(audusd_sim.id, BookType::L2_MBP);
    expected.apply_depth(&full).unwrap();
    let delta = expected.to_deltas(full.ts_event, full.ts_init);
    let mut truncated = full.clone();
    truncated.bids.truncate(2);
    truncated.asks.truncate(2);
    truncated.bid_counts.truncate(2);
    truncated.ask_counts.truncate(2);

    if depth_source {
        expected.apply_depth(&truncated).unwrap();
        engine.process_data(Data::BookDepth(Box::new(truncated.clone())));
        engine.process_data(Data::BookDeltas(Box::new(delta.clone())));
    } else {
        engine.process_data(Data::BookDeltas(Box::new(delta.clone())));
        engine.process_data(Data::BookDepth(Box::new(truncated.clone())));
    }

    let cache = engine.cache();
    let cache = cache.borrow();
    let book = cache.order_book(&audusd_sim.id).unwrap();
    assert_eq!(book.instrument_id, expected.instrument_id);
    assert_eq!(book.book_type, expected.book_type);
    assert_eq!(book.sequence, expected.sequence);
    assert_eq!(book.ts_last, expected.ts_last);
    assert_eq!(book.bids_as_map(None), expected.bids_as_map(None));
    assert_eq!(book.asks_as_map(None), expected.asks_as_map(None));
    assert_eq!(
        serde_json::to_value(depths.get_messages()).unwrap(),
        serde_json::to_value(vec![truncated]).unwrap()
    );
    assert_eq!(
        serde_json::to_value(deltas.get_messages()).unwrap(),
        serde_json::to_value(vec![delta]).unwrap()
    );
}

#[rstest]
#[case::deltas_unmanaged_first(false, true)]
#[case::deltas_unmanaged_last(false, false)]
#[case::depth_unmanaged_first(true, true)]
#[case::depth_unmanaged_last(true, false)]
fn test_managed_book_owners_release_independently(
    managed_book_engine: Rc<RefCell<DataEngine>>,
    audusd_sim: CurrencyPair,
    client_id: ClientId,
    #[case] depth_source: bool,
    #[case] unmanaged_first: bool,
) {
    let mut engine = managed_book_engine.borrow_mut();
    let first = book_source_command(audusd_sim.id, client_id, depth_source, true);
    let second = book_source_command(audusd_sim.id, client_id, depth_source, true);
    let unmanaged = book_source_command(audusd_sim.id, client_id, depth_source, false);
    for command in [&unmanaged, &first, &second] {
        engine.execute_subscribe(command.clone()).unwrap();
    }

    let release = |engine: &mut DataEngine, command: &SubscribeCommand| {
        engine
            .execute_unsubscribe(&command.clone().into_unsubscribe(
                UUID4::new(),
                UnixNanos::from(103),
                Some(command.command_id()),
            ))
            .unwrap();
    };

    let subscribers = || {
        if depth_source {
            msgbus::subscriber_count_depth(switchboard::get_book_depth_topic(audusd_sim.id))
        } else {
            msgbus::subscriber_count_deltas(switchboard::get_book_deltas_topic(audusd_sim.id))
        }
    };

    if unmanaged_first {
        release(&mut engine, &unmanaged);
        assert_eq!(subscribers(), 1);
    }

    release(&mut engine, &first);
    assert_eq!(subscribers(), 1);
    // Releasing the same owner twice must not remove the remaining owner.
    release(&mut engine, &first);
    assert_eq!(subscribers(), 1);
    release(&mut engine, &second);
    assert_eq!(subscribers(), 0);
    let replacement = book_source_command(audusd_sim.id, client_id, !depth_source, true);
    engine.execute_subscribe(replacement).unwrap();

    if !unmanaged_first {
        release(&mut engine, &unmanaged);
    }

    assert_eq!(subscribers(), 0);
    assert_eq!(
        if depth_source {
            msgbus::subscriber_count_deltas(switchboard::get_book_deltas_topic(audusd_sim.id))
        } else {
            msgbus::subscriber_count_depth(switchboard::get_book_depth_topic(audusd_sim.id))
        },
        1
    );
}

#[rstest]
#[case::book_type(0)]
#[case::depth(1)]
#[case::parameters(2)]
#[case::client(3)]
fn test_managed_book_rejects_incompatible_configuration(
    managed_book_engine: Rc<RefCell<DataEngine>>,
    audusd_sim: CurrencyPair,
    client_id: ClientId,
    #[case] conflict: u8,
) {
    let mut engine = managed_book_engine.borrow_mut();
    let owner = book_source_command(audusd_sim.id, client_id, false, true);
    engine.execute_subscribe(owner).unwrap();
    let mut incoming = book_source_command(audusd_sim.id, client_id, false, true);

    let SubscribeCommand::BookDeltas(cmd) = &mut incoming else {
        unreachable!()
    };

    match conflict {
        0 => cmd.book_type = BookType::L3_MBO,
        1 => cmd.depth = NonZeroUsize::new(50),
        2 => {
            let mut params = Params::new();
            params.insert("rpi".into(), serde_json::json!(true));
            cmd.params = Some(params);
        }
        3 => cmd.client_id = Some(ClientId::new("OTHER")),
        _ => unreachable!(),
    }

    let error = engine.execute_subscribe(incoming).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("same client, book type, depth, and parameters")
    );
    assert_eq!(
        msgbus::subscriber_count_deltas(switchboard::get_book_deltas_topic(audusd_sim.id)),
        1
    );
}

#[rstest]
#[case::depth_first(true)]
#[case::interval_first(false)]
fn test_interval_and_managed_depth_conflict(
    managed_book_engine: Rc<RefCell<DataEngine>>,
    audusd_sim: CurrencyPair,
    client_id: ClientId,
    #[case] depth_first: bool,
) {
    let mut engine = managed_book_engine.borrow_mut();
    let depth = book_source_command(audusd_sim.id, client_id, true, true);
    let interval = SubscribeCommand::BookSnapshots(SubscribeBookSnapshots::new(
        audusd_sim.id,
        BookType::L2_MBP,
        Some(client_id),
        Some(audusd_sim.id.venue),
        UUID4::new(),
        UnixNanos::from(101),
        NonZeroUsize::new(25),
        NonZeroUsize::new(100).unwrap(),
        None,
        None,
    ));

    let (first, second) = if depth_first {
        (depth, interval)
    } else {
        (interval, depth)
    };

    engine.execute_subscribe(first.clone()).unwrap();
    assert!(
        engine
            .execute_subscribe(second.clone())
            .unwrap_err()
            .to_string()
            .contains("Conflicting managed book source")
    );
    engine
        .execute_unsubscribe(&second.clone().into_unsubscribe(
            UUID4::new(),
            UnixNanos::from(103),
            Some(second.command_id()),
        ))
        .unwrap();
    assert_eq!(
        msgbus::subscriber_count_deltas(switchboard::get_book_deltas_topic(audusd_sim.id)),
        usize::from(!depth_first)
    );
    assert_eq!(
        msgbus::subscriber_count_depth(switchboard::get_book_depth_topic(audusd_sim.id)),
        usize::from(depth_first)
    );
    engine
        .execute_unsubscribe(&first.clone().into_unsubscribe(
            UUID4::new(),
            UnixNanos::from(107),
            Some(first.command_id()),
        ))
        .unwrap();
    engine.execute_subscribe(second).unwrap();
}

#[rstest]
fn test_parent_book_owner_keeps_original_targets(
    managed_book_engine: Rc<RefCell<DataEngine>>,
    client_id: ClientId,
) {
    let mut engine = managed_book_engine.borrow_mut();
    let cache = engine.cache().clone();
    let first = make_es_future("ESZ1.XCME", "ESZ1");
    let second = make_es_future("ESH2.XCME", "ESH2");
    let first_id = first.id();
    let second_id = second.id();
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::FuturesContract(first))
        .unwrap();
    let parent_id = InstrumentId::from("ES.FUT.XCME");
    let mut original = book_source_command(parent_id, client_id, false, true);

    let SubscribeCommand::BookDeltas(command) = &mut original else {
        unreachable!();
    };

    command.params = Some(parent_params());
    engine.execute_subscribe(original.clone()).unwrap();

    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::FuturesContract(second))
        .unwrap();
    let mut expanded = original.clone();

    let SubscribeCommand::BookDeltas(command) = &mut expanded else {
        unreachable!();
    };

    command.command_id = UUID4::new();
    engine.execute_subscribe(expanded.clone()).unwrap();
    assert_eq!(
        msgbus::subscriber_count_deltas(switchboard::get_book_deltas_topic(first_id)),
        1
    );
    assert_eq!(
        msgbus::subscriber_count_deltas(switchboard::get_book_deltas_topic(second_id)),
        1
    );

    let expanded_id = expanded.command_id();
    engine
        .execute_unsubscribe(&expanded.into_unsubscribe(
            UUID4::new(),
            UnixNanos::from(103),
            Some(expanded_id),
        ))
        .unwrap();
    assert_eq!(
        msgbus::subscriber_count_deltas(switchboard::get_book_deltas_topic(first_id)),
        1
    );
    assert_eq!(
        msgbus::subscriber_count_deltas(switchboard::get_book_deltas_topic(second_id)),
        0
    );

    engine
        .execute_subscribe(book_source_command(second_id, client_id, true, true))
        .unwrap();
    let original_id = original.command_id();
    engine
        .execute_unsubscribe(&original.into_unsubscribe(
            UUID4::new(),
            UnixNanos::from(107),
            Some(original_id),
        ))
        .unwrap();
    assert_eq!(
        msgbus::subscriber_count_deltas(switchboard::get_book_deltas_topic(first_id)),
        0
    );
    assert_eq!(
        msgbus::subscriber_count_deltas(switchboard::get_book_deltas_topic(second_id)),
        0
    );
    assert_eq!(
        msgbus::subscriber_count_depth(switchboard::get_book_depth_topic(second_id)),
        1
    );
}

#[rstest]
fn test_book_unsubscribe_without_correlation_releases_first_owner(
    managed_book_engine: Rc<RefCell<DataEngine>>,
    audusd_sim: CurrencyPair,
    client_id: ClientId,
) {
    let mut engine = managed_book_engine.borrow_mut();
    let unmanaged = book_source_command(audusd_sim.id, client_id, false, false);
    let managed = book_source_command(audusd_sim.id, client_id, false, true);
    engine.execute_subscribe(unmanaged.clone()).unwrap();
    engine.execute_subscribe(managed.clone()).unwrap();

    engine
        .execute_unsubscribe(&unmanaged.into_unsubscribe(UUID4::new(), UnixNanos::from(103), None))
        .unwrap();
    let topic = switchboard::get_book_deltas_topic(audusd_sim.id);
    assert_eq!(msgbus::subscriber_count_deltas(topic), 1);
    let managed_id = managed.command_id();
    engine
        .execute_unsubscribe(&managed.into_unsubscribe(
            UUID4::new(),
            UnixNanos::from(107),
            Some(managed_id),
        ))
        .unwrap();
    assert_eq!(msgbus::subscriber_count_deltas(topic), 0);
}

#[rstest]
#[case::book_type(0)]
#[case::smaller_depth(1)]
#[case::oversized_depth(2)]
#[case::rpi(3)]
fn test_unmanaged_depth_rejects_conflicting_feed_configuration(
    managed_book_engine: Rc<RefCell<DataEngine>>,
    audusd_sim: CurrencyPair,
    client_id: ClientId,
    #[case] conflict: u8,
) {
    let mut engine = managed_book_engine.borrow_mut();
    let mut owner = book_source_command(audusd_sim.id, client_id, true, false);

    let SubscribeCommand::BookDepth(command) = &mut owner else {
        unreachable!()
    };

    command.depth = NonZeroUsize::new(5);
    engine.execute_subscribe(owner.clone()).unwrap();

    let mut incoming = owner.clone();

    let SubscribeCommand::BookDepth(command) = &mut incoming else {
        unreachable!()
    };

    command.command_id = UUID4::new();

    match conflict {
        0 => command.book_type = BookType::L3_MBO,
        1 => command.depth = NonZeroUsize::new(1),
        2 => command.depth = NonZeroUsize::new(6),
        3 => {
            let mut params = Params::new();
            params.insert("rpi".into(), serde_json::json!(true));
            command.params = Some(params);
        }
        _ => unreachable!(),
    }

    let error = engine.execute_subscribe(incoming.clone()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("same client, book type, depth, and parameters")
    );
    let incoming_id = incoming.command_id();
    engine
        .execute_unsubscribe(&incoming.into_unsubscribe(
            UUID4::new(),
            UnixNanos::from(103),
            Some(incoming_id),
        ))
        .unwrap();
    assert_eq!(engine.subscribed_book_depth(), vec![audusd_sim.id]);
    let owner_id = owner.command_id();
    engine
        .execute_unsubscribe(&owner.into_unsubscribe(
            UUID4::new(),
            UnixNanos::from(107),
            Some(owner_id),
        ))
        .unwrap();
    assert!(engine.subscribed_book_depth().is_empty());
}

fn make_es_option(instrument_id: &str, symbol: &str, kind: OptionKind) -> OptionContract {
    OptionContract::builder()
        .instrument_id(InstrumentId::from(instrument_id))
        .raw_symbol(Symbol::from(symbol))
        .asset_class(AssetClass::Index)
        .exchange(Ustr::from("XCME"))
        .underlying(Ustr::from("ES"))
        .option_kind(kind)
        .strike_price(Price::from("4000.00"))
        .currency(Currency::USD())
        .activation_ns(UnixNanos::default())
        .expiration_ns(UnixNanos::from(2_000_000_000_000_000_000u64))
        .price_precision(2)
        .price_increment(Price::from("0.01"))
        .multiplier(Quantity::from(1))
        .lot_size(Quantity::from(1))
        .ts_event(UnixNanos::default())
        .ts_init(UnixNanos::default())
        .build()
        .unwrap()
}

fn book_source_command(
    id: InstrumentId,
    client: ClientId,
    depth_source: bool,
    managed: bool,
) -> SubscribeCommand {
    if depth_source {
        SubscribeCommand::BookDepth(SubscribeBookDepth::new(
            id,
            BookType::L2_MBP,
            Some(client),
            Some(id.venue),
            UUID4::new(),
            UnixNanos::from(101),
            NonZeroUsize::new(25),
            managed,
            None,
            None,
        ))
    } else {
        SubscribeCommand::BookDeltas(SubscribeBookDeltas::new(
            id,
            BookType::L2_MBP,
            Some(client),
            Some(id.venue),
            UUID4::new(),
            UnixNanos::from(101),
            NonZeroUsize::new(25),
            managed,
            None,
            None,
        ))
    }
}

#[derive(Clone, Copy, Debug)]
enum BookSubscriptionKind {
    Deltas,
    Depth,
}

impl BookSubscriptionKind {
    fn failure(self) -> MockSubscribeFailure {
        match self {
            Self::Deltas => MockSubscribeFailure::BookDeltas,
            Self::Depth => MockSubscribeFailure::BookDepth,
        }
    }
}

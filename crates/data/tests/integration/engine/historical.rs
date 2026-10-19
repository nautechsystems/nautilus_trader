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
fn test_response_trims_before_cache_write(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();
    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);

    // Send a bounded response with out-of-window leading and trailing rows.
    // Only the row at ts_init=2_000 should reach the cache.
    data_engine.response(leg_quotes_response(
        UUID4::new(),
        instrument_id,
        client_id,
        vec![
            pipeline_quote(instrument_id, 1_000),
            pipeline_quote(instrument_id, 2_000),
            pipeline_quote(instrument_id, 3_000),
        ],
        Some(UnixNanos::from(2_000)),
        Some(UnixNanos::from(2_000)),
    ));

    let cached = cache
        .borrow()
        .quotes(&instrument_id)
        .expect("cache must contain the trimmed quote");
    let ts_inits: Vec<u64> = cached.iter().map(|q| q.ts_init.as_u64()).collect();
    assert_eq!(ts_inits, vec![2_000]);
}

#[rstest]
fn test_book_response_skips_cache_write_when_subscription_active(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(Rc::clone(&clock), Rc::clone(&cache), None);

    let mock_client = MockDataClient::new(clock, Rc::clone(&cache), client_id, Some(venue));
    let adapter = DataClientAdapter::new(client_id, Some(venue), true, true, Box::new(mock_client));
    data_engine.register_client(adapter, None);

    let sub = SubscribeBookDeltas::new(
        instrument_id,
        BookType::L3_MBO,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        true,
        None,
        None,
    );
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::BookDeltas(sub)));

    let live_delta = OrderBookDeltaTestBuilder::new(instrument_id).build();
    data_engine.process_data(Data::BookDelta(live_delta));

    let maintained_count = cache
        .borrow()
        .order_book(&instrument_id)
        .expect("subscription must seed a cache book")
        .update_count;
    assert!(
        maintained_count > 0,
        "live delta must have advanced the cache book"
    );

    let fresh_book = OrderBook::new(instrument_id, BookType::L3_MBO);
    data_engine.response(book_response_for(
        UUID4::new(),
        instrument_id,
        client_id,
        fresh_book,
    ));

    let after_count = cache
        .borrow()
        .order_book(&instrument_id)
        .expect("cache book must remain after a book response")
        .update_count;
    assert_eq!(
        after_count, maintained_count,
        "book response must not clobber a book owned by a live subscription"
    );
}

#[rstest]
fn test_book_response_writes_to_cache_when_no_active_subscription(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);

    assert!(cache.borrow().order_book(&instrument_id).is_none());

    let fresh_book = OrderBook::new(instrument_id, BookType::L2_MBP);
    data_engine.response(book_response_for(
        UUID4::new(),
        instrument_id,
        client_id,
        fresh_book,
    ));

    assert!(
        cache.borrow().order_book(&instrument_id).is_some(),
        "without an active subscription the book response must populate the cache"
    );
}

#[rstest]
fn test_book_response_writes_to_cache_with_unmanaged_subscription(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(Rc::clone(&clock), Rc::clone(&cache), None);

    let mock_client = MockDataClient::new(clock, Rc::clone(&cache), client_id, Some(venue));
    let adapter = DataClientAdapter::new(client_id, Some(venue), true, true, Box::new(mock_client));
    data_engine.register_client(adapter, None);

    let sub = SubscribeBookDeltas::new(
        instrument_id,
        BookType::L3_MBO,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        false, // unmanaged
        None,
        None,
    );
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::BookDeltas(sub)));

    assert!(
        cache.borrow().order_book(&instrument_id).is_none(),
        "unmanaged subscriptions do not install a BookUpdater or seed the cache",
    );

    let fresh_book = OrderBook::new(instrument_id, BookType::L3_MBO);
    data_engine.response(book_response_for(
        UUID4::new(),
        instrument_id,
        client_id,
        fresh_book,
    ));

    assert!(
        cache.borrow().order_book(&instrument_id).is_some(),
        "unmanaged subscriptions must not gate snapshot population of the cache"
    );
}

#[rstest]
fn test_book_response_always_delivers_to_requester(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(Rc::clone(&clock), Rc::clone(&cache), None);

    let mock_client = MockDataClient::new(clock, cache, client_id, Some(venue));
    let adapter = DataClientAdapter::new(client_id, Some(venue), true, true, Box::new(mock_client));
    data_engine.register_client(adapter, None);

    let sub = SubscribeBookDeltas::new(
        instrument_id,
        BookType::L3_MBO,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        true,
        None,
        None,
    );
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::BookDeltas(sub)));

    let request_id = UUID4::new();
    let (handler, saver) =
        get_any_saving_handler::<BookResponse>(Some(Ustr::from("book-response-delivery")));
    msgbus::register_response_handler(&request_id, handler);

    let fresh_book = OrderBook::new(instrument_id, BookType::L3_MBO);
    data_engine.response(book_response_for(
        request_id,
        instrument_id,
        client_id,
        fresh_book,
    ));

    let received = saver.get_messages();
    assert_eq!(
        received.len(),
        1,
        "requester must receive the snapshot even when cache write is skipped"
    );
    assert_eq!(received[0].correlation_id, request_id);
}

#[rstest]
fn test_book_deltas_response_skips_cache_write_when_subscription_active(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(Rc::clone(&clock), Rc::clone(&cache), None);

    let mock_client = MockDataClient::new(clock, Rc::clone(&cache), client_id, Some(venue));
    let adapter = DataClientAdapter::new(client_id, Some(venue), true, true, Box::new(mock_client));
    data_engine.register_client(adapter, None);

    let sub = SubscribeBookDeltas::new(
        instrument_id,
        BookType::L3_MBO,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        true,
        None,
        None,
    );
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::BookDeltas(sub)));

    let live_delta = OrderBookDeltaTestBuilder::new(instrument_id).build();
    data_engine.process_data(Data::BookDelta(live_delta));
    let maintained_count = cache
        .borrow()
        .order_book(&instrument_id)
        .expect("managed sub must seed a cache book")
        .update_count;

    let request_id = UUID4::new();
    let (handler, saver) =
        get_any_saving_handler::<BookDeltasResponse>(Some(Ustr::from("deltas-response-skip")));
    msgbus::register_response_handler(&request_id, handler);

    data_engine.response(DataResponse::BookDeltas(BookDeltasResponse::new(
        request_id,
        client_id,
        instrument_id,
        vec![split_delta(instrument_id, 1_500)],
        None,
        None,
        UnixNanos::default(),
        None,
    )));

    let after_count = cache
        .borrow()
        .order_book(&instrument_id)
        .expect("cache book remains under active subscription")
        .update_count;
    assert_eq!(
        after_count, maintained_count,
        "historical deltas must not mutate a cache book owned by a live subscription"
    );

    let received = saver.get_messages();
    assert_eq!(received.len(), 1, "requester still receives the response");
    assert_eq!(received[0].correlation_id, request_id);
}

#[rstest]
fn test_book_depth_response_publishes_pipeline_depths(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache, None);

    let pipeline_topic =
        switchboard::MessagingSwitchboard::default().get_pipeline_book_depth_topic(instrument_id);
    let (handler, saver) =
        get_typed_message_saving_handler::<OrderBookDepth>(Some(Ustr::from("depth-response")));
    msgbus::subscribe_book_depth(pipeline_topic.into(), handler, None);

    data_engine.response(DataResponse::BookDepth(BookDepthResponse::new(
        UUID4::new(),
        client_id,
        instrument_id,
        vec![
            book_depth_at(instrument_id, 1_000),
            book_depth_at(instrument_id, 2_000),
        ],
        None,
        None,
        UnixNanos::default(),
        None,
    )));

    let depths = saver.get_messages();
    assert_eq!(depths.len(), 2);
    let ts_inits: Vec<u64> = depths.iter().map(|d| d.ts_init.as_u64()).collect();
    assert_eq!(ts_inits, vec![1_000, 2_000]);
}

#[rstest]
fn test_book_deltas_response_publishes_frames_by_f_last(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache, None);

    let pipeline_topic =
        switchboard::MessagingSwitchboard::default().get_pipeline_book_deltas_topic(instrument_id);
    let (handler, saver) =
        get_typed_message_saving_handler::<OrderBookDeltas>(Some(Ustr::from("deltas-by-f-last")));
    msgbus::subscribe_book_deltas(pipeline_topic.into(), handler, None);

    let f_last = RecordFlag::F_LAST as u8;
    let payload = vec![
        delta_with_flag(instrument_id, 1_000, 0),
        delta_with_flag(instrument_id, 2_000, f_last),
        delta_with_flag(instrument_id, 3_000, 0),
        delta_with_flag(instrument_id, 4_000, f_last),
        delta_with_flag(instrument_id, 5_000, 0),
    ];

    data_engine.response(DataResponse::BookDeltas(BookDeltasResponse::new(
        UUID4::new(),
        client_id,
        instrument_id,
        payload,
        None,
        None,
        UnixNanos::default(),
        None,
    )));

    let batches = saver.get_messages();
    assert_eq!(
        batches.len(),
        3,
        "two F_LAST-terminated frames plus a trailing partial must publish as three batches"
    );
    let frame_sizes: Vec<usize> = batches.iter().map(|b| b.deltas.len()).collect();
    assert_eq!(frame_sizes, vec![2, 2, 1]);
    let frame_end_ts: Vec<u64> = batches
        .iter()
        .map(|b| b.deltas.last().unwrap().ts_event.as_u64())
        .collect();
    assert_eq!(
        frame_end_ts,
        vec![2_000, 4_000, 5_000],
        "each batch must close on the F_LAST delta of its frame (or the trailing delta)"
    );
}

#[rstest]
fn test_book_deltas_response_applies_to_cache_when_no_subscription_but_book_exists(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);

    cache
        .borrow_mut()
        .add_order_book(OrderBook::new(instrument_id, BookType::L3_MBO))
        .unwrap();
    let before_count = cache
        .borrow()
        .order_book(&instrument_id)
        .expect("seeded book")
        .update_count;

    data_engine.response(DataResponse::BookDeltas(BookDeltasResponse::new(
        UUID4::new(),
        client_id,
        instrument_id,
        vec![
            split_delta(instrument_id, 1_000),
            split_delta(instrument_id, 2_000),
        ],
        None,
        None,
        UnixNanos::default(),
        None,
    )));

    let after_count = cache
        .borrow()
        .order_book(&instrument_id)
        .expect("book still present")
        .update_count;
    assert!(
        after_count > before_count,
        "historical deltas must apply to a cache book when no live subscription owns it (was {before_count}, now {after_count})"
    );
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_book_deltas_request_replays_day_start_snapshot(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);

    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();

    let f_snapshot = RecordFlag::F_SNAPSHOT as u8;
    let f_last = RecordFlag::F_LAST as u8;
    let _catalog_dir = register_deltas_catalog_with_deltas(
        &mut data_engine,
        "deltas-replay-assemble",
        &[
            book_replay_delta(instrument_id, 0, f_snapshot | f_last, "1.00000", 1),
            book_replay_delta(instrument_id, 500, 0, "1.00010", 2),
            book_replay_delta(instrument_id, 1_500, f_last, "1.00020", 3),
            book_replay_delta(instrument_id, 2_000, f_last, "1.00030", 4),
        ],
        Some((0, 2_000)),
    );

    let parent_id = UUID4::new();
    let (handler, saver) =
        get_any_saving_handler::<BookDeltasResponse>(Some(Ustr::from("deltas-replay-assemble")));
    msgbus::register_response_handler(&parent_id, handler);

    let req = RequestCommand::BookDeltas(RequestBookDeltas::new(
        instrument_id,
        Some(UnixNanos::from(1_000).to_datetime_utc()),
        Some(UnixNanos::from(3_000).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.execute_request(req).unwrap();

    let received = saver.get_messages();
    assert_eq!(received.len(), 1);
    let data = &received[0].data;

    // Pre-start deltas (ts 0, 500, 1500) collapse into one synthesized snapshot keyed at the
    // crossing delta (ts 1500); the post-start delta (ts 2000) is forwarded unchanged.
    let ts_inits: Vec<u64> = data.iter().map(|d| d.ts_init.as_u64()).collect();
    assert!(
        ts_inits.iter().all(|&t| t >= 1_000),
        "no pre-start deltas survive the replay, was {ts_inits:?}"
    );
    assert_eq!(
        ts_inits[0], 1_500,
        "snapshot keyed at the crossing delta ts"
    );
    assert_eq!(*ts_inits.last().unwrap(), 2_000);
    assert_eq!(
        data[0].action,
        BookAction::Clear,
        "snapshot opens with a clear"
    );
    assert!(
        RecordFlag::F_SNAPSHOT.matches(data[1].flags),
        "synthesized adds carry F_SNAPSHOT"
    );
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_book_deltas_request_skips_replay_without_snapshot_flag(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);

    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();

    let f_last = RecordFlag::F_LAST as u8;
    let _catalog_dir = register_deltas_catalog_with_deltas(
        &mut data_engine,
        "deltas-replay-noflag",
        &[
            book_replay_delta(instrument_id, 0, 0, "1.00000", 1),
            book_replay_delta(instrument_id, 1_500, f_last, "1.00020", 2),
            book_replay_delta(instrument_id, 2_000, f_last, "1.00030", 3),
        ],
        Some((0, 2_000)),
    );

    let parent_id = UUID4::new();
    let (handler, saver) =
        get_any_saving_handler::<BookDeltasResponse>(Some(Ustr::from("deltas-replay-noflag")));
    msgbus::register_response_handler(&parent_id, handler);

    let req = RequestCommand::BookDeltas(RequestBookDeltas::new(
        instrument_id,
        Some(UnixNanos::from(1_000).to_datetime_utc()),
        Some(UnixNanos::from(3_000).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.execute_request(req).unwrap();

    let received = saver.get_messages();
    assert_eq!(received.len(), 1);
    let data = &received[0].data;

    // First delta lacks F_SNAPSHOT, so no replay: data is forwarded and trimmed to [start, end].
    let ts_inits: Vec<u64> = data.iter().map(|d| d.ts_init.as_u64()).collect();
    assert_eq!(ts_inits, vec![1_500, 2_000]);
    assert_ne!(
        data[0].action,
        BookAction::Clear,
        "no snapshot was synthesized"
    );
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_book_deltas_request_skips_replay_when_snapshot_not_on_day_boundary(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);

    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();

    let f_snapshot = RecordFlag::F_SNAPSHOT as u8;
    let f_last = RecordFlag::F_LAST as u8;
    // The first snapshot delta sits at ts 500, not a UTC day boundary, so replay must bail.
    let _catalog_dir = register_deltas_catalog_with_deltas(
        &mut data_engine,
        "deltas-replay-offboundary",
        &[
            book_replay_delta(instrument_id, 500, f_snapshot | f_last, "1.00000", 1),
            book_replay_delta(instrument_id, 1_500, f_last, "1.00020", 2),
            book_replay_delta(instrument_id, 2_000, f_last, "1.00030", 3),
        ],
        Some((0, 2_000)),
    );

    let parent_id = UUID4::new();
    let (handler, saver) =
        get_any_saving_handler::<BookDeltasResponse>(Some(Ustr::from("deltas-replay-offboundary")));
    msgbus::register_response_handler(&parent_id, handler);

    let req = RequestCommand::BookDeltas(RequestBookDeltas::new(
        instrument_id,
        Some(UnixNanos::from(1_000).to_datetime_utc()),
        Some(UnixNanos::from(3_000).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.execute_request(req).unwrap();

    let received = saver.get_messages();
    assert_eq!(received.len(), 1);
    let data = &received[0].data;

    let ts_inits: Vec<u64> = data.iter().map(|d| d.ts_init.as_u64()).collect();
    assert_eq!(ts_inits, vec![1_500, 2_000]);
    assert_ne!(
        data[0].action,
        BookAction::Clear,
        "no snapshot was synthesized"
    );
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_book_deltas_request_skips_replay_when_start_at_day_boundary(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);

    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();

    let f_snapshot = RecordFlag::F_SNAPSHOT as u8;
    let f_last = RecordFlag::F_LAST as u8;
    let _catalog_dir = register_deltas_catalog_with_deltas(
        &mut data_engine,
        "deltas-replay-atboundary",
        &[
            book_replay_delta(instrument_id, 0, f_snapshot | f_last, "1.00000", 1),
            book_replay_delta(instrument_id, 1_500, f_last, "1.00020", 2),
        ],
        Some((0, 2_000)),
    );

    let parent_id = UUID4::new();
    let (handler, saver) =
        get_any_saving_handler::<BookDeltasResponse>(Some(Ustr::from("deltas-replay-atboundary")));
    msgbus::register_response_handler(&parent_id, handler);

    // Request starts exactly on the day boundary, so there is nothing to fast-forward.
    let req = RequestCommand::BookDeltas(RequestBookDeltas::new(
        instrument_id,
        Some(UnixNanos::from(0).to_datetime_utc()),
        Some(UnixNanos::from(3_000).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.execute_request(req).unwrap();

    let received = saver.get_messages();
    assert_eq!(received.len(), 1);
    let data = &received[0].data;

    let ts_inits: Vec<u64> = data.iter().map(|d| d.ts_init.as_u64()).collect();
    assert_eq!(ts_inits, vec![0, 1_500]);
    assert_eq!(
        data[0].action,
        BookAction::Add,
        "original day-start snapshot delta is preserved, not re-synthesized"
    );
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_book_deltas_request_replays_end_snapshot_when_exhausted(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);

    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();

    let f_snapshot = RecordFlag::F_SNAPSHOT as u8;
    let f_last = RecordFlag::F_LAST as u8;
    let _catalog_dir = register_deltas_catalog_with_deltas(
        &mut data_engine,
        "deltas-replay-exhausted",
        &[
            book_replay_delta(instrument_id, 0, f_snapshot | f_last, "1.00000", 1),
            book_replay_delta(instrument_id, 500, f_last, "1.00010", 2),
        ],
        Some((0, 500)),
    );

    let parent_id = UUID4::new();
    let (handler, saver) =
        get_any_saving_handler::<BookDeltasResponse>(Some(Ustr::from("deltas-replay-exhausted")));
    msgbus::register_response_handler(&parent_id, handler);

    // All catalog deltas precede the original start, so the end-state snapshot is keyed at it.
    let req = RequestCommand::BookDeltas(RequestBookDeltas::new(
        instrument_id,
        Some(UnixNanos::from(1_000).to_datetime_utc()),
        Some(UnixNanos::from(3_000).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.execute_request(req).unwrap();

    let received = saver.get_messages();
    assert_eq!(received.len(), 1);
    let data = &received[0].data;

    let ts_inits: Vec<u64> = data.iter().map(|d| d.ts_init.as_u64()).collect();
    assert!(
        ts_inits.iter().all(|&t| t == 1_000),
        "the synthesized end-state snapshot is keyed at the original start, was {ts_inits:?}"
    );
    assert_eq!(data[0].action, BookAction::Clear);
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_book_deltas_request_from_day_start_false_skips_floor(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);

    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();

    let f_snapshot = RecordFlag::F_SNAPSHOT as u8;
    let f_last = RecordFlag::F_LAST as u8;
    let _catalog_dir = register_deltas_catalog_with_deltas(
        &mut data_engine,
        "deltas-replay-nofloor",
        &[
            book_replay_delta(instrument_id, 0, f_snapshot | f_last, "1.00000", 1),
            book_replay_delta(instrument_id, 1_500, f_last, "1.00020", 2),
            book_replay_delta(instrument_id, 2_000, f_last, "1.00030", 3),
        ],
        Some((0, 2_000)),
    );

    let parent_id = UUID4::new();
    let (handler, saver) =
        get_any_saving_handler::<BookDeltasResponse>(Some(Ustr::from("deltas-replay-nofloor")));
    msgbus::register_response_handler(&parent_id, handler);

    let params: Params = serde_json::from_value(json!({ "from_day_start": false })).unwrap();
    let req = RequestCommand::BookDeltas(RequestBookDeltas::new(
        instrument_id,
        Some(UnixNanos::from(1_000).to_datetime_utc()),
        Some(UnixNanos::from(3_000).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        Some(params),
    ));
    data_engine.execute_request(req).unwrap();

    let received = saver.get_messages();
    assert_eq!(received.len(), 1);
    let data = &received[0].data;

    // Without the day-start floor the catalog read never returns the ts-0 snapshot frame, so the
    // first in-window delta lacks F_SNAPSHOT and no replay occurs.
    let ts_inits: Vec<u64> = data.iter().map(|d| d.ts_init.as_u64()).collect();
    assert_eq!(ts_inits, vec![1_500, 2_000]);
    assert_ne!(data[0].action, BookAction::Clear);
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_book_deltas_replay_writes_assembled_snapshot_to_cache(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);

    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();
    cache
        .borrow_mut()
        .add_order_book(OrderBook::new(instrument_id, BookType::L2_MBP))
        .unwrap();

    let f_snapshot = RecordFlag::F_SNAPSHOT as u8;
    let f_last = RecordFlag::F_LAST as u8;
    let _catalog_dir = register_deltas_catalog_with_deltas(
        &mut data_engine,
        "deltas-replay-cache",
        &[
            book_replay_delta(instrument_id, 0, f_snapshot | f_last, "1.00000", 1),
            book_replay_delta(instrument_id, 1_500, f_last, "1.00020", 2),
        ],
        Some((0, 2_000)),
    );

    let parent_id = UUID4::new();
    let (handler, _saver) =
        get_any_saving_handler::<BookDeltasResponse>(Some(Ustr::from("deltas-replay-cache")));
    msgbus::register_response_handler(&parent_id, handler);

    let req = RequestCommand::BookDeltas(RequestBookDeltas::new(
        instrument_id,
        Some(UnixNanos::from(1_000).to_datetime_utc()),
        Some(UnixNanos::from(3_000).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.execute_request(req).unwrap();

    // No live subscription owns the book, so the assembled snapshot is applied to the cache.
    let cache_ref = cache.borrow();
    let book = cache_ref
        .order_book(&instrument_id)
        .expect("seeded book present");
    assert!(
        book.update_count > 0,
        "replayed snapshot must mutate the cache book"
    );
    assert!(
        book.best_ask_price().is_some(),
        "snapshot levels reach the cache book"
    );
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_book_deltas_replay_respects_cache_ownership(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(Rc::clone(&clock), Rc::clone(&cache), None);

    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();

    let mock_client = MockDataClient::new(clock, Rc::clone(&cache), client_id, Some(venue));
    let adapter = DataClientAdapter::new(client_id, Some(venue), true, true, Box::new(mock_client));
    data_engine.register_client(adapter, None);

    let sub = SubscribeBookDeltas::new(
        instrument_id,
        BookType::L3_MBO,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        true,
        None,
        None,
    );
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::BookDeltas(sub)));

    let live_delta = OrderBookDeltaTestBuilder::new(instrument_id).build();
    data_engine.process_data(Data::BookDelta(live_delta));
    let owned_count = cache
        .borrow()
        .order_book(&instrument_id)
        .expect("managed sub seeds a cache book")
        .update_count;

    let f_snapshot = RecordFlag::F_SNAPSHOT as u8;
    let f_last = RecordFlag::F_LAST as u8;
    let _catalog_dir = register_deltas_catalog_with_deltas(
        &mut data_engine,
        "deltas-replay-owned",
        &[
            book_replay_delta(instrument_id, 0, f_snapshot | f_last, "1.00000", 1),
            book_replay_delta(instrument_id, 1_500, f_last, "1.00020", 2),
        ],
        Some((0, 2_000)),
    );

    let parent_id = UUID4::new();
    let (handler, _saver) =
        get_any_saving_handler::<BookDeltasResponse>(Some(Ustr::from("deltas-replay-owned")));
    msgbus::register_response_handler(&parent_id, handler);

    let req = RequestCommand::BookDeltas(RequestBookDeltas::new(
        instrument_id,
        Some(UnixNanos::from(1_000).to_datetime_utc()),
        Some(UnixNanos::from(3_000).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.execute_request(req).unwrap();

    let after_count = cache
        .borrow()
        .order_book(&instrument_id)
        .expect("cache book remains under active subscription")
        .update_count;
    assert_eq!(
        after_count, owned_count,
        "replayed snapshot must not mutate a cache book owned by a live subscription"
    );
}

fn book_response_for(
    request_id: UUID4,
    instrument_id: InstrumentId,
    client_id: ClientId,
    book: OrderBook,
) -> DataResponse {
    DataResponse::Book(BookResponse::new(
        request_id,
        client_id,
        instrument_id,
        book,
        None,
        None,
        UnixNanos::default(),
        None,
    ))
}

#[cfg(feature = "streaming")]
fn book_replay_delta(
    instrument_id: InstrumentId,
    ts: u64,
    flags: u8,
    price: &str,
    order_id: u64,
) -> OrderBookDelta {
    OrderBookDeltaTestBuilder::new(instrument_id)
        .book_order(BookOrder::new(
            OrderSide::Sell,
            Price::from(price),
            Quantity::from("1"),
            order_id,
        ))
        .flags(flags)
        .sequence(order_id)
        .ts_event(UnixNanos::from(ts))
        .ts_init(UnixNanos::from(ts))
        .build()
}

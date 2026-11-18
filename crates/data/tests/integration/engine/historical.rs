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
fn test_mixed_instrument_historical_quotes_cache_by_row(
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let a = InstrumentId::from("AUDUSD.SIM");
    let b = InstrumentId::from("EURUSD.SIM");
    let quotes = [pipeline_quote(a, 1_000), pipeline_quote(b, 2_000)];
    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);

    data_engine.response(leg_quotes_response(
        UUID4::new(),
        a,
        client_id,
        quotes.to_vec(),
        None,
        None,
    ));

    assert_eq!(cache.borrow().quotes(&a), Some(vec![quotes[0]]));
    assert_eq!(cache.borrow().quotes(&b), Some(vec![quotes[1]]));
}

#[rstest]
#[case::envelope(false)]
#[case::partial_failure(true)]
fn test_response_envelope_preserves_root_and_source_correlation(
    #[case] failure: bool,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    data_engine: Rc<RefCell<DataEngine>>,
) {
    let root = UUID4::new();
    let child = UUID4::new();

    let response = QuotesResponse::new(
        child,
        ClientId::test_default(),
        InstrumentId::from("AUD/USD.SIM"),
        Vec::new(),
        None,
        None,
        UnixNanos::from(31),
        None,
    );
    let envelope = DataResponseEnvelope::new(DataResponse::Quotes(response), true, Some(child))
        .with_failure("Missing sibling".to_owned());
    let mut wire = serde_json::to_value(envelope).unwrap();
    wire["correlation_id"] = serde_json::to_value(root).unwrap();
    let envelope = serde_json::from_value(wire).unwrap();
    let received = Rc::new(RefCell::new(Vec::new()));
    let shared = Rc::clone(&received);
    msgbus::register_response_handler(
        &root,
        TypedHandler::from(move |reply: &DataResponseEnvelope| {
            shared.borrow_mut().push(reply.clone());
        }),
    );

    if failure {
        data_engine.borrow_mut().request_failed(DataRequestFailed {
            request_id: root,
            reason: "Missing sibling".to_owned(),
            response: Some(DataResponse::Quotes(QuotesResponse::new(
                child,
                ClientId::test_default(),
                InstrumentId::from("AUD/USD.SIM"),
                Vec::new(),
                None,
                None,
                UnixNanos::from(31),
                None,
            ))),
        });
    } else {
        data_engine.borrow_mut().response_envelope(envelope);
    }

    let received = received.borrow();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].correlation_id(), &root);
    assert_eq!(
        received[0].sub_correlation_id(),
        (!failure).then_some(child)
    );
    assert_eq!(received[0].response().unwrap().correlation_id(), &child);
    assert!(received[0].completed());
    assert_eq!(received[0].failure_reason(), Some("Missing sibling"));
    assert!(stub_msgbus.borrow().get_response_handler(&root).is_none());
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
        get_response_saving_handler::<CustomDataResponse>(Some(Ustr::from("custom-data-response")));
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
        get_response_saving_handler::<CustomDataResponse>(Some(Ustr::from("custom-response-only")));
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
        get_response_saving_handler::<BookResponse>(Some(Ustr::from("book-response-delivery")));
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
        get_response_saving_handler::<BookDeltasResponse>(Some(Ustr::from("deltas-response-skip")));
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

    let request_id = UUID4::new();
    let envelopes = save_response_envelopes(&request_id);
    let f_last = RecordFlag::F_LAST as u8;
    let payload = vec![
        delta_with_flag(instrument_id, 1_000, 0),
        delta_with_flag(instrument_id, 2_000, f_last),
        delta_with_flag(instrument_id, 3_000, 0),
        delta_with_flag(instrument_id, 4_000, f_last),
        delta_with_flag(instrument_id, 5_000, 0),
    ];

    data_engine.response(DataResponse::BookDeltas(BookDeltasResponse::new(
        request_id,
        client_id,
        instrument_id,
        payload.clone(),
        None,
        None,
        UnixNanos::default(),
        None,
    )));

    let batches = saver.get_messages();
    let frame_sizes: Vec<usize> = batches.iter().map(|b| b.deltas.len()).collect();
    assert_eq!(frame_sizes, vec![2, 2]);
    let frame_end_ts: Vec<u64> = batches
        .iter()
        .map(|b| b.deltas.last().unwrap().ts_event.as_u64())
        .collect();
    assert_eq!(
        frame_end_ts,
        vec![2_000, 4_000],
        "each batch must close on the F_LAST delta of its frame"
    );
    assert_eq!(
        failure_reasons(&envelopes),
        vec![Some(
            "Order book deltas end without an `F_LAST` delta for AUD/USD.SIM \
             (1 pending, ts_init 5000 to 5000)"
                .to_owned()
        )],
        "a trailing partial event fails the request and stays unpublished"
    );
    assert_eq!(
        delivered_book_deltas(&envelopes),
        vec![payload[..4].to_vec()],
        "the requester receives only the complete events"
    );
}

#[rstest]
fn test_book_deltas_response_carries_open_event_across_envelopes(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache, None);
    let saver = subscribe_pipeline_book_deltas(instrument_id, "deltas-carry-envelopes");

    let request_id = UUID4::new();
    let envelopes = save_response_envelopes(&request_id);
    let f_last = RecordFlag::F_LAST as u8;
    let first = delta_with_flag(instrument_id, 1_000, 0);
    let second = delta_with_flag(instrument_id, 2_000, f_last);
    let third = delta_with_flag(instrument_id, 3_000, f_last);

    data_engine.response_envelope(book_deltas_envelope(
        request_id,
        client_id,
        instrument_id,
        vec![first],
        false,
    ));
    data_engine.response_envelope(book_deltas_envelope(
        request_id,
        client_id,
        instrument_id,
        vec![second, third],
        true,
    ));

    assert_eq!(
        published_events(&saver),
        vec![vec![first, second], vec![third]],
        "an open event must wait for its F_LAST delta"
    );
    assert_eq!(
        delivered_book_deltas(&envelopes),
        vec![vec![first, second, third]],
        "the requester receives the open event with the envelope that closes it"
    );
    assert_eq!(failure_reasons(&envelopes), vec![None]);
}

#[rstest]
fn test_book_deltas_response_carries_open_events_per_instrument(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let first_id = audusd_sim.id;
    let second_id = InstrumentId::from("GBP/USD.SIM");
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache, None);
    let first_saver = subscribe_pipeline_book_deltas(first_id, "deltas-carry-first");
    let second_saver = subscribe_pipeline_book_deltas(second_id, "deltas-carry-second");

    let request_id = UUID4::new();
    let envelopes = save_response_envelopes(&request_id);
    let f_last = RecordFlag::F_LAST as u8;
    let first_open = delta_with_flag(first_id, 1_000, 0);
    let second_event = delta_with_flag(second_id, 2_000, f_last);
    let second_open = delta_with_flag(second_id, 3_000, 0);
    let first_close = delta_with_flag(first_id, 4_000, f_last);

    data_engine.response_envelope(book_deltas_envelope(
        request_id,
        client_id,
        first_id,
        vec![first_open],
        false,
    ));
    data_engine.response_envelope(book_deltas_envelope(
        request_id,
        client_id,
        second_id,
        vec![second_event, second_open],
        false,
    ));
    data_engine.response_envelope(book_deltas_envelope(
        request_id,
        client_id,
        first_id,
        vec![first_close],
        true,
    ));

    assert_eq!(
        published_events(&first_saver),
        vec![vec![first_open, first_close]]
    );
    assert_eq!(published_events(&second_saver), vec![vec![second_event]]);
    assert_eq!(
        delivered_book_deltas(&envelopes),
        vec![vec![second_event], vec![first_open, first_close]]
    );
    assert_eq!(
        failure_reasons(&envelopes),
        vec![
            None,
            Some(
                "Order book deltas end without an `F_LAST` delta for GBP/USD.SIM \
                 (1 pending, ts_init 3000 to 3000)"
                    .to_owned()
            ),
        ],
        "completion fails on the event the other instrument left open"
    );
}

#[rstest]
fn test_book_deltas_response_keeps_open_events_per_request(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache, None);
    let saver = subscribe_pipeline_book_deltas(instrument_id, "deltas-carry-requests");

    let request_id = UUID4::new();
    let envelopes = save_response_envelopes(&request_id);
    let f_last = RecordFlag::F_LAST as u8;
    let open = delta_with_flag(instrument_id, 1_000, 0);
    let other_request_event = delta_with_flag(instrument_id, 2_000, f_last);
    let close = delta_with_flag(instrument_id, 3_000, f_last);

    data_engine.response_envelope(book_deltas_envelope(
        request_id,
        client_id,
        instrument_id,
        vec![open],
        false,
    ));
    data_engine.response_envelope(book_deltas_envelope(
        UUID4::new(),
        client_id,
        instrument_id,
        vec![other_request_event],
        true,
    ));
    data_engine.response_envelope(book_deltas_envelope(
        request_id,
        client_id,
        instrument_id,
        vec![close],
        true,
    ));

    assert_eq!(
        published_events(&saver),
        vec![vec![other_request_event], vec![open, close]]
    );
    assert_eq!(delivered_book_deltas(&envelopes), vec![vec![open, close]]);
}

#[rstest]
#[case::completed(
    None,
    "Order book deltas end without an `F_LAST` delta for AUD/USD.SIM \
     (1 pending, ts_init 2000 to 2000)"
)]
#[case::failed(
    Some("Dataset unavailable"),
    "Dataset unavailable; Order book deltas end without an `F_LAST` delta for AUD/USD.SIM \
     (1 pending, ts_init 2000 to 2000)"
)]
fn test_book_deltas_response_fails_request_ending_mid_event(
    #[case] reason: Option<&str>,
    #[case] expected: &str,
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache, None);
    let saver = subscribe_pipeline_book_deltas(instrument_id, "deltas-carry-strict");

    let request_id = UUID4::new();
    let envelopes = save_response_envelopes(&request_id);
    let closed = delta_with_flag(instrument_id, 1_000, RecordFlag::F_LAST as u8);
    let open = delta_with_flag(instrument_id, 2_000, 0);

    data_engine.response_envelope(book_deltas_envelope(
        request_id,
        client_id,
        instrument_id,
        vec![closed, open],
        false,
    ));
    data_engine.response_envelope(DataResponseEnvelope::terminal(
        request_id,
        reason.map(str::to_owned),
    ));

    assert_eq!(published_events(&saver), vec![vec![closed]]);
    assert_eq!(
        delivered_book_deltas(&envelopes),
        vec![vec![closed], vec![]],
        "the open event never reaches the requester"
    );
    assert_eq!(
        failure_reasons(&envelopes),
        vec![None, Some(expected.to_owned())]
    );
}

#[rstest]
#[case::split_before_snapshot(true)]
#[case::single_payload(false)]
fn test_book_deltas_response_publishes_open_event_before_snapshot(
    #[case] split: bool,
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache, None);
    let saver = subscribe_pipeline_book_deltas(instrument_id, "deltas-carry-snapshot");

    let request_id = UUID4::new();
    let envelopes = save_response_envelopes(&request_id);
    let open = delta_with_flag(instrument_id, 1_000, 0);
    let snapshot_clear = OrderBookDelta::clear(
        instrument_id,
        0,
        UnixNanos::from(2_000),
        UnixNanos::from(2_000),
    );
    let snapshot_add = delta_with_flag(
        instrument_id,
        2_000,
        RecordFlag::F_SNAPSHOT as u8 | RecordFlag::F_LAST as u8,
    );

    if split {
        data_engine.response_envelope(book_deltas_envelope(
            request_id,
            client_id,
            instrument_id,
            vec![open],
            false,
        ));
        data_engine.response_envelope(book_deltas_envelope(
            request_id,
            client_id,
            instrument_id,
            vec![snapshot_clear, snapshot_add],
            true,
        ));
    } else {
        data_engine.response_envelope(book_deltas_envelope(
            request_id,
            client_id,
            instrument_id,
            vec![open, snapshot_clear, snapshot_add],
            true,
        ));
    }

    assert_eq!(
        published_events(&saver),
        vec![vec![open], vec![snapshot_clear, snapshot_add]]
    );
    assert_eq!(
        delivered_book_deltas(&envelopes),
        vec![vec![open, snapshot_clear, snapshot_add]]
    );
    assert_eq!(failure_reasons(&envelopes), vec![None]);
}

#[rstest]
#[case::split_before_snapshot(true)]
#[case::single_payload(false)]
fn test_book_deltas_response_withholds_open_event_before_snapshot_after_f_last(
    #[case] split: bool,
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), None);
    let saver = subscribe_pipeline_book_deltas(instrument_id, "deltas-withhold-snapshot");

    cache
        .borrow_mut()
        .add_order_book(OrderBook::new(instrument_id, BookType::L3_MBO))
        .unwrap();

    let request_id = UUID4::new();
    let envelopes = save_response_envelopes(&request_id);
    let closed = delta_with_flag(instrument_id, 1_000, RecordFlag::F_LAST as u8);
    let open = delta_with_flag(instrument_id, 2_000, 0);
    let snapshot_clear = OrderBookDelta::clear(
        instrument_id,
        0,
        UnixNanos::from(3_000),
        UnixNanos::from(3_000),
    );
    let snapshot_add = delta_with_flag(
        instrument_id,
        3_000,
        RecordFlag::F_SNAPSHOT as u8 | RecordFlag::F_LAST as u8,
    );
    let after = delta_with_flag(instrument_id, 4_000, RecordFlag::F_LAST as u8);

    if split {
        data_engine.response_envelope(book_deltas_envelope(
            request_id,
            client_id,
            instrument_id,
            vec![closed, open],
            false,
        ));
        data_engine.response_envelope(book_deltas_envelope(
            request_id,
            client_id,
            instrument_id,
            vec![snapshot_clear, snapshot_add, after],
            true,
        ));
    } else {
        data_engine.response_envelope(book_deltas_envelope(
            request_id,
            client_id,
            instrument_id,
            vec![closed, open, snapshot_clear, snapshot_add, after],
            true,
        ));
    }

    let mut reasons = failure_reasons(&envelopes);
    let reason = reasons.pop();

    assert_eq!(
        published_events(&saver),
        vec![
            vec![closed],
            vec![snapshot_clear, snapshot_add],
            vec![after]
        ]
    );
    assert_eq!(
        delivered_book_deltas(&envelopes).concat(),
        vec![closed, snapshot_clear, snapshot_add, after],
        "the open event never reaches the requester"
    );
    assert_eq!(
        cache
            .borrow()
            .order_book(&instrument_id)
            .expect("seeded book")
            .update_count,
        4,
        "the open event must not reach the cache book"
    );
    assert_eq!(
        reason,
        Some(Some(
            "Order book deltas reach a snapshot without an `F_LAST` delta for AUD/USD.SIM \
             (1 pending, ts_init 2000 to 2000)"
                .to_owned()
        ))
    );
    assert_eq!(reasons, vec![None; usize::from(split)]);
}

#[rstest]
fn test_book_deltas_response_reports_events_open_at_snapshot_and_at_end(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache, None);
    let saver = subscribe_pipeline_book_deltas(instrument_id, "deltas-withhold-both");

    let request_id = UUID4::new();
    let envelopes = save_response_envelopes(&request_id);
    let closed = delta_with_flag(instrument_id, 1_000, RecordFlag::F_LAST as u8);

    let open_at_snapshot = [
        delta_with_flag(instrument_id, 2_000, 0),
        delta_with_flag(instrument_id, 2_500, 0),
    ];

    let snapshot_clear = OrderBookDelta::clear(
        instrument_id,
        0,
        UnixNanos::from(3_000),
        UnixNanos::from(3_000),
    );
    let snapshot_add = delta_with_flag(
        instrument_id,
        3_000,
        RecordFlag::F_SNAPSHOT as u8 | RecordFlag::F_LAST as u8,
    );
    let open_at_end = delta_with_flag(instrument_id, 4_000, 0);

    data_engine.response_envelope(book_deltas_envelope(
        request_id,
        client_id,
        instrument_id,
        vec![
            closed,
            open_at_snapshot[0],
            open_at_snapshot[1],
            snapshot_clear,
            snapshot_add,
            open_at_end,
        ],
        true,
    ));

    assert_eq!(
        published_events(&saver),
        vec![vec![closed], vec![snapshot_clear, snapshot_add]]
    );
    assert_eq!(
        delivered_book_deltas(&envelopes),
        vec![vec![closed, snapshot_clear, snapshot_add]]
    );
    assert_eq!(
        failure_reasons(&envelopes),
        vec![Some(
            "Order book deltas reach a snapshot without an `F_LAST` delta for AUD/USD.SIM \
             (2 pending, ts_init 2000 to 2500); Order book deltas end without an `F_LAST` delta \
             for AUD/USD.SIM (1 pending, ts_init 4000 to 4000)"
                .to_owned()
        )]
    );
}

#[rstest]
fn test_book_deltas_response_keeps_open_event_through_book_clear(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache, None);
    let saver = subscribe_pipeline_book_deltas(instrument_id, "deltas-carry-book-clear");

    let request_id = UUID4::new();
    let envelopes = save_response_envelopes(&request_id);
    let open = delta_with_flag(instrument_id, 1_000, 0);
    let mut book_clear = OrderBookDelta::clear(
        instrument_id,
        0,
        UnixNanos::from(2_000),
        UnixNanos::from(2_000),
    );
    book_clear.flags = RecordFlag::F_LAST as u8;

    data_engine.response_envelope(book_deltas_envelope(
        request_id,
        client_id,
        instrument_id,
        vec![open],
        false,
    ));
    data_engine.response_envelope(book_deltas_envelope(
        request_id,
        client_id,
        instrument_id,
        vec![book_clear],
        true,
    ));

    assert_eq!(published_events(&saver), vec![vec![open, book_clear]]);
    assert_eq!(
        delivered_book_deltas(&envelopes),
        vec![vec![open, book_clear]]
    );
    assert_eq!(failure_reasons(&envelopes), vec![None]);
}

#[rstest]
#[case::populated_snapshot(RecordFlag::F_SNAPSHOT as u8 | RecordFlag::F_LAST as u8)]
#[case::empty_snapshot(RecordFlag::F_LAST as u8)]
fn test_book_deltas_response_publishes_open_event_after_snapshot_without_other_f_last(
    #[case] snapshot_end_flags: u8,
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache, None);
    let saver = subscribe_pipeline_book_deltas(instrument_id, "deltas-carry-unflagged");

    let request_id = UUID4::new();
    let envelopes = save_response_envelopes(&request_id);
    let mut snapshot_clear = OrderBookDelta::clear(
        instrument_id,
        0,
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
    );

    let snapshot: Vec<OrderBookDelta> = if RecordFlag::F_SNAPSHOT.matches(snapshot_end_flags) {
        vec![
            snapshot_clear,
            delta_with_flag(instrument_id, 1_000, snapshot_end_flags),
        ]
    } else {
        snapshot_clear.flags = snapshot_end_flags;
        vec![snapshot_clear]
    };

    let first = delta_with_flag(instrument_id, 2_000, 0);
    let second = delta_with_flag(instrument_id, 3_000, 0);
    let mut payload = snapshot.clone();
    payload.extend([first, second]);

    data_engine.response_envelope(book_deltas_envelope(
        request_id,
        client_id,
        instrument_id,
        payload.clone(),
        true,
    ));

    assert_eq!(
        published_events(&saver),
        vec![snapshot, vec![first, second]]
    );
    assert_eq!(delivered_book_deltas(&envelopes), vec![payload]);
    assert_eq!(failure_reasons(&envelopes), vec![None]);
}

#[rstest]
fn test_book_deltas_response_drops_open_event_on_reset(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache, None);
    let saver = subscribe_pipeline_book_deltas(instrument_id, "deltas-carry-reset");

    let request_id = UUID4::new();
    let open = delta_with_flag(instrument_id, 1_000, 0);
    let close = delta_with_flag(instrument_id, 2_000, RecordFlag::F_LAST as u8);

    data_engine.response_envelope(book_deltas_envelope(
        request_id,
        client_id,
        instrument_id,
        vec![open],
        false,
    ));
    data_engine.reset();
    data_engine.response_envelope(book_deltas_envelope(
        request_id,
        client_id,
        instrument_id,
        vec![close],
        true,
    ));

    assert_eq!(published_events(&saver), vec![vec![close]]);
}

#[rstest]
#[case::below_limit(9_999, false)]
#[case::at_limit(10_000, true)]
#[case::above_limit(10_001, true)]
fn test_book_deltas_response_bounds_open_event_without_f_last(
    #[case] held: u64,
    #[case] releases_before_completion: bool,
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache, None);
    let saver = subscribe_pipeline_book_deltas(instrument_id, "deltas-carry-limit");

    let request_id = UUID4::new();
    let envelopes = save_response_envelopes(&request_id);
    let mut deltas: Vec<OrderBookDelta> = (1..=held)
        .map(|ts| delta_with_flag(instrument_id, ts, 0))
        .collect();
    let last = delta_with_flag(instrument_id, held + 1, 0);

    data_engine.response_envelope(book_deltas_envelope(
        request_id,
        client_id,
        instrument_id,
        deltas.clone(),
        false,
    ));
    let published_before_completion = published_events(&saver);
    let delivered_before_completion = delivered_book_deltas(&envelopes);

    data_engine.response_envelope(book_deltas_envelope(
        request_id,
        client_id,
        instrument_id,
        vec![last],
        true,
    ));

    let expected = if releases_before_completion {
        vec![deltas, vec![last]]
    } else {
        deltas.push(last);
        vec![deltas]
    };

    let expected_before_completion = &expected[..usize::from(releases_before_completion)];
    assert_eq!(published_before_completion, expected_before_completion);
    assert_eq!(delivered_before_completion, expected_before_completion);
    assert_eq!(published_events(&saver), expected);
    assert_eq!(delivered_book_deltas(&envelopes), expected);
}

#[rstest]
fn test_book_deltas_response_holds_open_event_past_limit_after_f_last(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache, None);
    let saver = subscribe_pipeline_book_deltas(instrument_id, "deltas-carry-past-limit");

    let request_id = UUID4::new();
    let envelopes = save_response_envelopes(&request_id);
    let f_last = RecordFlag::F_LAST as u8;
    let mut deltas = vec![delta_with_flag(instrument_id, 1, f_last)];
    deltas.extend((2..=10_001).map(|ts| delta_with_flag(instrument_id, ts, 0)));

    data_engine.response_envelope(book_deltas_envelope(
        request_id,
        client_id,
        instrument_id,
        deltas,
        false,
    ));
    data_engine.response_envelope(book_deltas_envelope(
        request_id,
        client_id,
        instrument_id,
        vec![delta_with_flag(instrument_id, 10_002, f_last)],
        true,
    ));

    let sizes: Vec<usize> = saver
        .get_messages()
        .iter()
        .map(|b| b.deltas.len())
        .collect();
    let delivered_sizes: Vec<usize> = delivered_book_deltas(&envelopes)
        .iter()
        .map(Vec::len)
        .collect();
    assert_eq!(sizes, vec![1, 10_001]);
    assert_eq!(delivered_sizes, vec![1, 10_001]);
    assert_eq!(failure_reasons(&envelopes), vec![None, None]);
}

#[rstest]
#[case::completed(None)]
#[case::failed(Some("Dataset unavailable"))]
fn test_book_deltas_response_publishes_open_event_on_terminal_envelope(
    #[case] reason: Option<&str>,
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache, None);
    let saver = subscribe_pipeline_book_deltas(instrument_id, "deltas-carry-terminal");

    let request_id = UUID4::new();
    let envelopes = save_response_envelopes(&request_id);
    let open = delta_with_flag(instrument_id, 1_000, 0);

    data_engine.response_envelope(book_deltas_envelope(
        request_id,
        client_id,
        instrument_id,
        vec![open],
        false,
    ));
    let published_before_terminal = saver.get_messages().len();

    data_engine.response_envelope(DataResponseEnvelope::terminal(
        request_id,
        reason.map(str::to_owned),
    ));

    let completed: Vec<bool> = envelopes
        .get_messages()
        .iter()
        .map(DataResponseEnvelope::completed)
        .collect();
    assert_eq!(published_before_terminal, 0);
    assert_eq!(published_events(&saver), vec![vec![open]]);
    assert_eq!(
        delivered_book_deltas(&envelopes),
        vec![vec![open], vec![]],
        "the open event reaches the requester ahead of the terminal envelope"
    );
    assert_eq!(completed, vec![false, true]);
    assert_eq!(
        failure_reasons(&envelopes),
        vec![None, reason.map(str::to_owned)]
    );
}

#[rstest]
fn test_book_deltas_response_holds_open_event_from_cache(
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

    let cache_update_count = || {
        cache
            .borrow()
            .order_book(&instrument_id)
            .expect("seeded book")
            .update_count
    };

    let request_id = UUID4::new();
    let f_last = RecordFlag::F_LAST as u8;

    data_engine.response_envelope(book_deltas_envelope(
        request_id,
        client_id,
        instrument_id,
        vec![
            delta_with_flag(instrument_id, 1_000, f_last),
            delta_with_flag(instrument_id, 2_000, 0),
        ],
        false,
    ));
    let count_while_open = cache_update_count();

    data_engine.response_envelope(book_deltas_envelope(
        request_id,
        client_id,
        instrument_id,
        vec![delta_with_flag(instrument_id, 3_000, f_last)],
        true,
    ));

    assert_eq!(
        count_while_open, 1,
        "an open event must not reach the cache book"
    );
    assert_eq!(cache_update_count(), 3);
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
    let next_event = book_replay_delta(instrument_id, 1_500, f_last, "1.00020", 3);
    let later_event = book_replay_delta(instrument_id, 2_000, f_last, "1.00030", 4);
    let _catalog_dir = register_deltas_catalog_with_deltas(
        &mut data_engine,
        "deltas-replay-assemble",
        &[
            book_replay_delta(instrument_id, 0, f_snapshot | f_last, "1.00000", 1),
            book_replay_delta(instrument_id, 500, f_last, "1.00010", 2),
            next_event,
            later_event,
        ],
        Some((0, 2_000)),
    );

    let parent_id = UUID4::new();
    let (handler, saver) = get_response_saving_handler::<BookDeltasResponse>(Some(Ustr::from(
        "deltas-replay-assemble",
    )));
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

    // Pre-start events (ts 0, 500) collapse into one synthesized snapshot keyed at the start
    // (ts 1000); the events from the start (ts 1500, 2000) are forwarded unchanged.
    let ts_inits: Vec<u64> = data.iter().map(|d| d.ts_init.as_u64()).collect();
    let ts_events: Vec<u64> = data.iter().map(|d| d.ts_event.as_u64()).collect();
    let snapshot_prices: Vec<Price> = data[1..3].iter().map(|d| d.order.price).collect();
    assert_eq!(ts_inits, vec![1_000, 1_000, 1_000, 1_500, 2_000]);
    assert_eq!(ts_events, ts_inits);
    assert_eq!(
        snapshot_prices,
        vec![Price::from("1.00000"), Price::from("1.00010")]
    );
    assert_eq!(
        data[0].action,
        BookAction::Clear,
        "snapshot opens with a clear"
    );
    assert_eq!(
        data[1].flags, f_snapshot,
        "synthesized adds carry F_SNAPSHOT"
    );
    assert_eq!(
        data[2].flags,
        f_snapshot | f_last,
        "snapshot closes as one event"
    );
    assert_eq!(data[3], next_event);
    assert_eq!(data[4], later_event);
}

#[cfg(feature = "streaming")]
#[rstest]
#[case::start_between_events(1_000)]
#[case::start_at_event(1_500)]
fn test_book_deltas_request_forwards_events_from_start_unchanged(
    #[case] start: u64,
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
    let event_open = book_replay_delta(instrument_id, 1_500, 0, "1.00020", 3);
    let event_close = book_replay_delta(instrument_id, 1_500, f_last, "1.00030", 4);
    let later_event = book_replay_delta(instrument_id, 2_000, f_last, "1.00040", 5);
    let _catalog_dir = register_deltas_catalog_with_deltas(
        &mut data_engine,
        "deltas-replay-whole-event",
        &[
            book_replay_delta(instrument_id, 0, f_snapshot | f_last, "1.00000", 1),
            book_replay_delta(instrument_id, 500, f_last, "1.00010", 2),
            event_open,
            event_close,
            later_event,
        ],
        Some((0, 2_000)),
    );

    let parent_id = UUID4::new();
    let (handler, saver) = get_response_saving_handler::<BookDeltasResponse>(Some(Ustr::from(
        "deltas-replay-whole-event",
    )));
    msgbus::register_response_handler(&parent_id, handler);

    let req = RequestCommand::BookDeltas(RequestBookDeltas::new(
        instrument_id,
        Some(UnixNanos::from(start).to_datetime_utc()),
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

    // The snapshot holds only the events before the start; the two-delta event at ts 1500 is
    // forwarded whole, including when the request starts exactly on it.
    let ts_inits: Vec<u64> = data.iter().map(|d| d.ts_init.as_u64()).collect();
    let snapshot_prices: Vec<Price> = data[1..3].iter().map(|d| d.order.price).collect();
    assert_eq!(ts_inits, vec![start, start, start, 1_500, 1_500, 2_000]);
    assert_eq!(
        snapshot_prices,
        vec![Price::from("1.00000"), Price::from("1.00010")]
    );
    let suffix = [event_open, event_close, later_event];
    assert_eq!(data[3..], suffix);

    for (actual, expected) in data[3..].iter().zip(suffix) {
        assert_eq!(actual.order.side, expected.order.side);
        assert_eq!(actual.order.price, expected.order.price);
        assert_eq!(actual.order.size, expected.order.size);
    }
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_book_deltas_time_range_request_replays_snapshot_for_first_window_only(
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
    let event_open = book_replay_delta(instrument_id, 1_500_000_000, 0, "1.00020", 3);
    let event_close = book_replay_delta(instrument_id, 2_500_000_000, f_last, "1.00030", 4);
    let later_event = book_replay_delta(instrument_id, 2_800_000_000, f_last, "1.00040", 5);
    let _catalog_dir = register_deltas_catalog_with_deltas(
        &mut data_engine,
        "deltas-replay-windows",
        &[
            book_replay_delta(instrument_id, 0, f_snapshot | f_last, "1.00000", 1),
            book_replay_delta(instrument_id, 500_000_000, f_last, "1.00010", 2),
            event_open,
            event_close,
            later_event,
        ],
        Some((0, 3_000_000_000)),
    );
    let saver = subscribe_pipeline_book_deltas(instrument_id, "deltas-replay-windows");

    let parent_id = UUID4::new();
    let envelopes = save_response_envelopes(&parent_id);
    let params: Params = serde_json::from_value(json!({
        "time_range_generator": "",
        "durations_seconds": [1],
    }))
    .unwrap();
    let req = RequestCommand::BookDeltas(RequestBookDeltas::new(
        instrument_id,
        Some(UnixNanos::from(1_000_000_000).to_datetime_utc()),
        Some(UnixNanos::from(3_000_000_000).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        Some(params),
    ));
    data_engine.execute_request(req).unwrap();

    // The first window opens with the snapshot stamped at the request start; the second window
    // continues its deltas, so the event straddling the window boundary publishes whole and
    // reaches the requester with the window that closes it.
    let payload_ts: Vec<Vec<u64>> = envelopes
        .get_messages()
        .iter()
        .filter_map(|envelope| match envelope.response() {
            Some(DataResponse::BookDeltas(r)) => {
                Some(r.data.iter().map(|d| d.ts_init.as_u64()).collect())
            }
            _ => None,
        })
        .collect();

    let events = published_events(&saver);
    let snapshot_ts: Vec<u64> = events[0].iter().map(|d| d.ts_init.as_u64()).collect();
    assert_eq!(
        payload_ts,
        vec![
            vec![1_000_000_000, 1_000_000_000, 1_000_000_000],
            vec![1_500_000_000, 2_500_000_000, 2_800_000_000],
            vec![],
        ]
    );
    assert_eq!(snapshot_ts, vec![1_000_000_000; 3]);
    assert_eq!(
        events[1..],
        [vec![event_open, event_close], vec![later_event]]
    );
    assert_eq!(failure_reasons(&envelopes), vec![None, None, None]);
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
        get_response_saving_handler::<BookDeltasResponse>(Some(Ustr::from("deltas-replay-noflag")));
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
    let (handler, saver) = get_response_saving_handler::<BookDeltasResponse>(Some(Ustr::from(
        "deltas-replay-offboundary",
    )));
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
    let (handler, saver) = get_response_saving_handler::<BookDeltasResponse>(Some(Ustr::from(
        "deltas-replay-atboundary",
    )));
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
    let (handler, saver) = get_response_saving_handler::<BookDeltasResponse>(Some(Ustr::from(
        "deltas-replay-exhausted",
    )));
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
    let (handler, saver) = get_response_saving_handler::<BookDeltasResponse>(Some(Ustr::from(
        "deltas-replay-nofloor",
    )));
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
        get_response_saving_handler::<BookDeltasResponse>(Some(Ustr::from("deltas-replay-cache")));
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
        get_response_saving_handler::<BookDeltasResponse>(Some(Ustr::from("deltas-replay-owned")));
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

fn subscribe_pipeline_book_deltas(
    instrument_id: InstrumentId,
    handler_id: &str,
) -> TypedMessageSavingHandler<OrderBookDeltas> {
    let topic =
        switchboard::MessagingSwitchboard::default().get_pipeline_book_deltas_topic(instrument_id);
    let (handler, saver) =
        get_typed_message_saving_handler::<OrderBookDeltas>(Some(Ustr::from(handler_id)));
    msgbus::subscribe_book_deltas(topic.into(), handler, None);
    saver
}

fn published_events(
    saver: &TypedMessageSavingHandler<OrderBookDeltas>,
) -> Vec<Vec<OrderBookDelta>> {
    saver
        .get_messages()
        .iter()
        .map(|batch| batch.deltas.clone())
        .collect()
}

fn save_response_envelopes(request_id: &UUID4) -> TypedMessageSavingHandler<DataResponseEnvelope> {
    let (handler, saver) = get_typed_message_saving_handler::<DataResponseEnvelope>(None);
    msgbus::register_response_handler(request_id, handler);
    saver
}

fn failure_reasons(saver: &TypedMessageSavingHandler<DataResponseEnvelope>) -> Vec<Option<String>> {
    saver
        .get_messages()
        .iter()
        .map(|envelope| envelope.failure_reason().map(str::to_owned))
        .collect()
}

fn book_deltas_envelope(
    request_id: UUID4,
    client_id: ClientId,
    instrument_id: InstrumentId,
    deltas: Vec<OrderBookDelta>,
    completed: bool,
) -> DataResponseEnvelope {
    let response = DataResponse::BookDeltas(BookDeltasResponse::new(
        request_id,
        client_id,
        instrument_id,
        deltas,
        None,
        None,
        UnixNanos::default(),
        None,
    ));
    DataResponseEnvelope::new(response, completed, None)
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

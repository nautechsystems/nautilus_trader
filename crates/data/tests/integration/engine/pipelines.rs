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
fn test_process_pipeline_quote_publishes_on_pipeline_topic_only(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
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
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
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
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
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
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
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
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
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
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
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
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
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
    data_engine.process_pipeline(Data::MarkPrice(mark_price));

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
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
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
    data_engine.process_pipeline(Data::IndexPrice(index_price));

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
fn test_process_pipeline_funding_rate_publishes_on_pipeline_topic_only(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let instrument_id = audusd_sim.id;
    let mut data_engine = DataEngine::new(clock, cache.clone(), None);

    let live_topic = switchboard::get_funding_rate_topic(instrument_id);
    let pipeline_topic_str = pipeline_topic_of(live_topic.as_ref());
    let pipeline_topic: MStr<Topic> = pipeline_topic_str.as_str().into();

    let (live_handler, live_saver) = get_typed_message_saving_handler::<FundingRateUpdate>(Some(
        Ustr::from("pipeline-funding-live"),
    ));
    let (pipeline_handler, pipeline_saver) = get_typed_message_saving_handler::<FundingRateUpdate>(
        Some(Ustr::from("pipeline-funding-pipeline")),
    );
    msgbus::subscribe_funding_rates(live_topic.into(), live_handler, None);
    msgbus::subscribe_funding_rates(pipeline_topic.into(), pipeline_handler, None);

    let funding_rate = FundingRateUpdate::new(
        instrument_id,
        "0.0001".parse().unwrap(),
        None,
        None,
        UnixNanos::from(1),
        UnixNanos::from(2),
    );
    data_engine.process_pipeline(Data::FundingRate(funding_rate));

    assert!(
        live_saver.get_messages().is_empty(),
        "pipeline funding rate must not publish on the live topic",
    );
    let pipeline_messages = pipeline_saver.get_messages();
    assert_eq!(pipeline_messages.len(), 1);
    assert_eq!(pipeline_messages[0], funding_rate);
    assert_eq!(
        cache.borrow().funding_rate(&instrument_id),
        Some(&funding_rate),
        "pipeline funding rate must populate the cache by default",
    );
}

#[rstest]
fn test_process_pipeline_instrument_status_publishes_on_pipeline_topic_only(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
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
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
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
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
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

    data_engine.process_pipeline(Data::BookDelta(delta));

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
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
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

    data_engine.process_pipeline(Data::BookDeltas(Box::new(deltas.clone())));

    assert!(
        live_saver.get_messages().is_empty(),
        "pipeline deltas must not publish on the live topic",
    );
    let pipeline_messages = pipeline_saver.get_messages();
    assert_eq!(pipeline_messages.len(), 1);
    assert_eq!(pipeline_messages[0], deltas);
}

#[rstest]
fn test_process_pipeline_depth_publishes_on_pipeline_topic_only(
    stub_msgbus: Rc<RefCell<MessageBus>>,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache, None);

    let depth = stub_depth10();
    let instrument_id = depth.instrument_id;
    let live_topic = switchboard::get_book_depth_topic(instrument_id);
    let pipeline_topic_str = pipeline_topic_of(live_topic.as_ref());
    let pipeline_topic: MStr<Topic> = pipeline_topic_str.as_str().into();

    let (live_handler, live_saver) =
        get_typed_message_saving_handler::<OrderBookDepth>(Some(Ustr::from("pipeline-depth-live")));
    let (pipeline_handler, pipeline_saver) = get_typed_message_saving_handler::<OrderBookDepth>(
        Some(Ustr::from("pipeline-depth-pipeline")),
    );
    msgbus::subscribe_book_depth(live_topic.into(), live_handler, None);
    msgbus::subscribe_book_depth(pipeline_topic.into(), pipeline_handler, None);

    data_engine.process_pipeline(Data::BookDepth(Box::new(depth.clone())));

    assert!(
        live_saver.get_messages().is_empty(),
        "pipeline depth must not publish on the live topic",
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
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
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
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
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
        "pipeline bar handler must honor validate_data_sequence and keep the first bar",
    );
}

#[rstest]
fn test_process_pipeline_skips_synthetic_quote_republish(stub_msgbus: Rc<RefCell<MessageBus>>) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
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
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
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
fn test_process_pipeline_depth_skips_derived_quote_emission(stub_msgbus: Rc<RefCell<MessageBus>>) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
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

    data_engine.process_pipeline(Data::BookDepth(Box::new(depth)));

    assert!(
        saver.get_messages().is_empty(),
        "pipeline depth must not emit a derived quote even when emit_quotes_from_book_depths is set",
    );
    assert!(
        cache.borrow().quote(&instrument_id).is_none(),
        "no derived quote should be cached for pipeline depth",
    );
}

#[rstest]
fn test_time_range_pipeline_issues_one_child_at_a_time(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_test_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(clock.clone(), cache.clone(), None);
    let recorder = register_time_range_recorder(&mut data_engine, clock, cache, client_id, venue);

    let parent_id = UUID4::new();
    let params: Params = serde_json::from_value(json!({
        "time_range_generator": "",
        "durations_seconds": [2],
    }))
    .unwrap();
    let req = RequestCommand::Quotes(RequestQuotes::new(
        instrument_id,
        Some(UnixNanos::from(1_000_000_000).to_datetime_utc()),
        Some(UnixNanos::from(5_000_000_000).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        Some(params),
    ));
    data_engine.execute_request(req).unwrap();

    let recorded = recorded_time_range_request_quotes(&recorder);
    assert_eq!(recorded.len(), 1);
    assert_ne!(recorded[0].request_id, parent_id);
    assert_eq!(
        recorded[0].start.map(|dt| dt.as_nanosecond()),
        Some(1_000_000_000)
    );
    assert_eq!(
        recorded[0].end.map(|dt| dt.as_nanosecond()),
        Some(3_000_000_000)
    );
    assert_eq!(data_engine.time_range_pipeline_count(), 1);

    data_engine.response(time_range_quote_response(
        &recorded[0],
        instrument_id,
        client_id,
        1,
        vec![pipeline_quote(instrument_id, 2_000_000_000)],
    ));

    let recorded = recorded_time_range_request_quotes(&recorder);
    assert_eq!(
        recorded.len(),
        2,
        "second child should be issued only after the first response"
    );
    assert_eq!(
        recorded[1].start.map(|dt| dt.as_nanosecond()),
        Some(3_000_000_001)
    );
    assert_eq!(
        recorded[1].end.map(|dt| dt.as_nanosecond()),
        Some(5_000_000_000)
    );
}

#[rstest]
fn test_time_range_pipeline_uses_data_count_feedback(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_test_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(clock.clone(), cache.clone(), None);
    let recorder = register_time_range_recorder(&mut data_engine, clock, cache, client_id, venue);

    let params: Params = serde_json::from_value(json!({
        "time_range_generator": "",
        "durations_seconds": [1, 3],
    }))
    .unwrap();
    let req = RequestCommand::Quotes(RequestQuotes::new(
        instrument_id,
        Some(UnixNanos::from(1_000_000_000).to_datetime_utc()),
        Some(UnixNanos::from(8_000_000_000).to_datetime_utc()),
        None,
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        Some(params),
    ));
    data_engine.execute_request(req).unwrap();

    let first = recorded_time_range_request_quotes(&recorder)[0].clone();
    data_engine.response(time_range_quote_response(
        &first,
        instrument_id,
        client_id,
        0,
        Vec::new(),
    ));

    let recorded = recorded_time_range_request_quotes(&recorder);
    assert_eq!(recorded.len(), 2);
    assert_eq!(
        recorded[1].start.map(|dt| dt.as_nanosecond()),
        Some(2_000_000_001)
    );
    assert_eq!(
        recorded[1].end.map(|dt| dt.as_nanosecond()),
        Some(5_000_000_000)
    );

    data_engine.response(time_range_quote_response(
        &recorded[1],
        instrument_id,
        client_id,
        4,
        Vec::new(),
    ));

    let recorded = recorded_time_range_request_quotes(&recorder);
    assert_eq!(recorded.len(), 3);
    assert_eq!(
        recorded[2].start.map(|dt| dt.as_nanosecond()),
        Some(5_000_000_001)
    );
    assert_eq!(
        recorded[2].end.map(|dt| dt.as_nanosecond()),
        Some(6_000_000_000)
    );
}

#[rstest]
fn test_time_range_pipeline_point_data_uses_single_point_windows(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_test_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(clock.clone(), cache.clone(), None);
    let recorder = register_time_range_recorder(&mut data_engine, clock, cache, client_id, venue);

    let parent_id = UUID4::new();
    let (handler, saver) =
        get_any_saving_handler::<QuotesResponse>(Some(Ustr::from("time-range-point-parent")));
    msgbus::register_response_handler(&parent_id, handler);

    let params: Params = serde_json::from_value(json!({
        "time_range_generator": "",
        "durations_seconds": [2],
        "point_data": true,
    }))
    .unwrap();
    let req = RequestCommand::Quotes(RequestQuotes::new(
        instrument_id,
        Some(UnixNanos::from(1_000_000_000).to_datetime_utc()),
        Some(UnixNanos::from(6_000_000_000).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        Some(params),
    ));
    data_engine.execute_request(req).unwrap();

    let first = recorded_time_range_request_quotes(&recorder)[0].clone();
    assert_eq!(
        first.start.map(|dt| dt.as_nanosecond()),
        Some(1_000_000_000)
    );
    assert_eq!(first.end.map(|dt| dt.as_nanosecond()), Some(1_000_000_000));

    data_engine.response(time_range_quote_response(
        &first,
        instrument_id,
        client_id,
        1,
        vec![pipeline_quote(instrument_id, 1_000_000_000)],
    ));

    let recorded = recorded_time_range_request_quotes(&recorder);
    assert_eq!(recorded.len(), 2);
    assert_eq!(
        recorded[1].start.map(|dt| dt.as_nanosecond()),
        Some(3_000_000_000)
    );
    assert_eq!(
        recorded[1].end.map(|dt| dt.as_nanosecond()),
        Some(3_000_000_000)
    );

    data_engine.response(time_range_quote_response(
        &recorded[1],
        instrument_id,
        client_id,
        1,
        vec![pipeline_quote(instrument_id, 3_000_000_000)],
    ));

    let recorded = recorded_time_range_request_quotes(&recorder);
    assert_eq!(recorded.len(), 3);
    assert_eq!(
        recorded[2].start.map(|dt| dt.as_nanosecond()),
        Some(5_000_000_000)
    );
    assert_eq!(
        recorded[2].end.map(|dt| dt.as_nanosecond()),
        Some(5_000_000_000)
    );

    data_engine.response(time_range_quote_response(
        &recorded[2],
        instrument_id,
        client_id,
        1,
        vec![pipeline_quote(instrument_id, 5_000_000_000)],
    ));

    let recorded = recorded_time_range_request_quotes(&recorder);
    assert_eq!(recorded.len(), 4);
    assert_eq!(
        recorded[3].start.map(|dt| dt.as_nanosecond()),
        Some(6_000_000_000)
    );
    assert_eq!(
        recorded[3].end.map(|dt| dt.as_nanosecond()),
        Some(6_000_000_000)
    );

    data_engine.response(time_range_quote_response(
        &recorded[3],
        instrument_id,
        client_id,
        1,
        vec![pipeline_quote(instrument_id, 6_000_000_000)],
    ));

    let received = saver.get_messages();
    assert_eq!(recorded_time_range_request_quotes(&recorder).len(), 4);
    assert_eq!(data_engine.time_range_pipeline_count(), 0);
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].correlation_id, parent_id);
    assert!(received[0].data.is_empty());
    assert_eq!(
        received[0]
            .params
            .as_ref()
            .and_then(|params| params.get("data_count"))
            .and_then(Value::as_u64),
        Some(4)
    );
}

#[rstest]
fn test_time_range_pipeline_updates_parent_request_bar_aggregation(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();
    advance_test_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(clock.clone(), cache.clone(), None);
    let recorder =
        register_time_range_recorder(&mut data_engine, clock, cache.clone(), client_id, venue);

    let bar_type = BarType::from(format!("{instrument_id}-1-SECOND-LAST-INTERNAL").as_str());
    let parent_id = UUID4::new();
    let (handler, saver) =
        get_any_saving_handler::<TradesResponse>(Some(Ustr::from("time-range-agg-parent")));
    msgbus::register_response_handler(&parent_id, handler);

    let params: Params = serde_json::from_value(json!({
        "time_range_generator": "",
        "bar_types": [bar_type.to_string()],
        "update_subscriptions": false,
    }))
    .unwrap();
    let req = RequestCommand::Trades(RequestTrades::new(
        instrument_id,
        Some(UnixNanos::from(0).to_datetime_utc()),
        Some(UnixNanos::from(2_000_000_000).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        Some(params),
    ));
    data_engine.execute_request(req).unwrap();

    let child = recorded_time_range_request_trades(&recorder)[0].clone();
    let child_params = child.params.as_ref().expect("child params must be present");
    assert!(!child_params.contains_key("time_range_generator"));
    assert!(!child_params.contains_key("bar_types"));

    data_engine.response(time_range_trade_response(
        &child,
        instrument_id,
        client_id,
        2,
        vec![
            make_trade(instrument_id, "0.65000", 1000, "time-range-1", 0),
            make_trade(
                instrument_id,
                "0.65010",
                1000,
                "time-range-2",
                1_000_000_000,
            ),
        ],
    ));

    assert_eq!(
        cache.borrow().bar(&bar_type).map(|bar| bar.ts_event),
        Some(UnixNanos::from(1_000_000_000)),
        "parent request aggregator must consume time-range child trade data"
    );

    let received = saver.get_messages();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].correlation_id, parent_id);
    assert!(received[0].data.is_empty());
    assert_eq!(
        received[0]
            .params
            .as_ref()
            .and_then(|params| params.get("data_count"))
            .and_then(Value::as_u64),
        Some(2)
    );

    let follow_up_params: Params = serde_json::from_value(json!({
        "bar_types": [bar_type.to_string()],
        "update_subscriptions": false,
    }))
    .unwrap();
    let follow_up = RequestCommand::Trades(RequestTrades::new(
        instrument_id,
        Some(UnixNanos::from(3_000_000_000).to_datetime_utc()),
        Some(UnixNanos::from(4_000_000_000).to_datetime_utc()),
        None,
        Some(client_id),
        UUID4::new(),
        UnixNanos::default(),
        Some(follow_up_params),
    ));
    data_engine
        .execute_request(follow_up)
        .expect("empty parent response must clean up parent request aggregators");
}

#[rstest]
fn test_time_range_pipeline_emits_empty_parent_response(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_test_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(clock.clone(), cache.clone(), None);
    let recorder = register_time_range_recorder(&mut data_engine, clock, cache, client_id, venue);

    let parent_id = UUID4::new();
    let (handler, saver) =
        get_any_saving_handler::<QuotesResponse>(Some(Ustr::from("time-range-parent")));
    msgbus::register_response_handler(&parent_id, handler);

    let params: Params = serde_json::from_value(json!({"time_range_generator": ""})).unwrap();
    let req = RequestCommand::Quotes(RequestQuotes::new(
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

    let child = recorded_time_range_request_quotes(&recorder)[0].clone();
    data_engine.response(time_range_quote_response(
        &child,
        instrument_id,
        client_id,
        2,
        Vec::new(),
    ));

    let received = saver.get_messages();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].correlation_id, parent_id);
    assert!(received[0].data.is_empty());
    assert_eq!(
        received[0]
            .params
            .as_ref()
            .and_then(|params| params.get("data_count"))
            .and_then(Value::as_u64),
        Some(2)
    );
    assert_eq!(data_engine.time_range_pipeline_count(), 0);
}

#[rstest]
fn test_reset_clears_time_range_pipeline_state(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_test_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(clock.clone(), cache.clone(), None);
    let recorder = register_time_range_recorder(&mut data_engine, clock, cache, client_id, venue);

    let parent_id = UUID4::new();
    let (parent_handler, parent_saver) =
        get_any_saving_handler::<QuotesResponse>(Some(Ustr::from("time-range-reset-parent")));
    msgbus::register_response_handler(&parent_id, parent_handler);

    let params: Params = serde_json::from_value(json!({
        "time_range_generator": "",
        "durations_seconds": [2],
    }))
    .unwrap();
    let req = RequestCommand::Quotes(RequestQuotes::new(
        instrument_id,
        Some(UnixNanos::from(1_000_000_000).to_datetime_utc()),
        Some(UnixNanos::from(5_000_000_000).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        Some(params),
    ));
    data_engine.execute_request(req).unwrap();

    let child = recorded_time_range_request_quotes(&recorder)[0].clone();
    let (child_handler, child_saver) =
        get_any_saving_handler::<QuotesResponse>(Some(Ustr::from("time-range-reset-child")));
    msgbus::register_response_handler(&child.request_id, child_handler);

    assert_eq!(data_engine.time_range_pipeline_count(), 1);
    data_engine.reset();
    assert_eq!(data_engine.time_range_pipeline_count(), 0);

    data_engine.response(time_range_quote_response(
        &child,
        instrument_id,
        client_id,
        1,
        vec![pipeline_quote(instrument_id, 2_000_000_000)],
    ));

    assert!(
        parent_saver.get_messages().is_empty(),
        "reset must clear time-range child mappings so no parent response fires",
    );
    assert_eq!(child_saver.get_messages().len(), 1);
    assert_eq!(
        child_saver.get_messages()[0].correlation_id,
        child.request_id
    );
}

#[rstest]
fn test_time_range_pipeline_request_join_runs_end_to_end(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_test_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(clock, cache.clone(), None);

    let leg_a = UUID4::new();
    let leg_b = UUID4::new();
    let parent_id = UUID4::new();
    let (handler, saver) =
        get_any_saving_handler::<QuotesResponse>(Some(Ustr::from("time-range-join-parent")));
    msgbus::register_response_handler(&parent_id, handler);

    let params: Params = serde_json::from_value(json!({
        "time_range_generator": "",
        "durations_seconds": [2],
    }))
    .unwrap();

    let join = RequestJoin::new(
        vec![leg_a, leg_b],
        Some(UnixNanos::from(1_000_000_000).to_datetime_utc()),
        Some(UnixNanos::from(5_000_000_000).to_datetime_utc()),
        parent_id,
        UnixNanos::default(),
        Some(params),
        None,
    );
    data_engine
        .execute_request(RequestCommand::Join(join))
        .unwrap();

    assert_eq!(data_engine.time_range_pipeline_count(), 1);
    assert_eq!(data_engine.pending_join_request_count(), 1);

    data_engine.response(leg_quotes_response(
        leg_a,
        instrument_id,
        client_id,
        vec![pipeline_quote(instrument_id, 1_500_000_000)],
        None,
        None,
    ));
    data_engine.response(leg_quotes_response(
        leg_b,
        instrument_id,
        client_id,
        vec![pipeline_quote(instrument_id, 2_500_000_000)],
        None,
        None,
    ));

    assert_eq!(data_engine.time_range_pipeline_count(), 1);
    assert_eq!(data_engine.pending_join_request_count(), 1);
    assert!(
        saver.get_messages().is_empty(),
        "parent callback must wait for the final empty response"
    );

    data_engine.response(leg_quotes_response(
        leg_a,
        instrument_id,
        client_id,
        vec![pipeline_quote(instrument_id, 3_500_000_000)],
        None,
        None,
    ));
    data_engine.response(leg_quotes_response(
        leg_b,
        instrument_id,
        client_id,
        vec![pipeline_quote(instrument_id, 4_500_000_000)],
        None,
        None,
    ));

    let received = saver.get_messages();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].correlation_id, parent_id);
    assert!(received[0].data.is_empty());
    assert_eq!(
        received[0]
            .params
            .as_ref()
            .and_then(|params| params.get("data_count"))
            .and_then(Value::as_u64),
        Some(4)
    );
    assert_eq!(data_engine.time_range_pipeline_count(), 0);
    assert_eq!(data_engine.pending_join_request_count(), 0);
    assert_eq!(
        cache
            .borrow()
            .quote(&instrument_id)
            .map(|quote| quote.ts_init),
        Some(UnixNanos::from(4_500_000_000))
    );
}

#[rstest]
fn test_time_range_pipeline_request_join_rejects_empty_window(
    stub_msgbus: Rc<RefCell<MessageBus>>,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_test_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(clock, cache, None);

    let params: Params = serde_json::from_value(json!({
        "time_range_generator": "",
        "durations_seconds": [2],
    }))
    .unwrap();

    let join = RequestJoin::new(
        vec![UUID4::new()],
        Some(UnixNanos::from(5_000_000_000).to_datetime_utc()),
        Some(UnixNanos::from(1_000_000_000).to_datetime_utc()),
        UUID4::new(),
        UnixNanos::default(),
        Some(params),
        None,
    );

    let err = data_engine
        .execute_request(RequestCommand::Join(join))
        .expect_err("empty-window time-range RequestJoin must fail fast");
    let err_message = err.to_string();
    assert!(
        err_message.contains("without a child window"),
        "error must explain why the Join cannot complete, was {err_message}"
    );
    assert_eq!(data_engine.time_range_pipeline_count(), 0);
    assert_eq!(data_engine.pending_join_request_count(), 0);
}

#[rstest]
fn test_time_range_pipeline_supports_bars_variant(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let bar_type = BarType::from(format!("{}-1-MINUTE-LAST-EXTERNAL", audusd_sim.id).as_str());
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_test_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(clock.clone(), cache.clone(), None);
    let recorder =
        register_time_range_recorder(&mut data_engine, clock, cache.clone(), client_id, venue);

    let parent_id = UUID4::new();
    let (handler, saver) =
        get_any_saving_handler::<BarsResponse>(Some(Ustr::from("time-range-bars-parent")));
    msgbus::register_response_handler(&parent_id, handler);

    let params: Params = serde_json::from_value(json!({"time_range_generator": ""})).unwrap();
    let req = RequestCommand::Bars(RequestBars::new(
        bar_type,
        Some(UnixNanos::from(1_000_000_000).to_datetime_utc()),
        Some(UnixNanos::from(2_000_000_000).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        Some(params),
    ));
    data_engine.execute_request(req).unwrap();

    let child = recorded_time_range_request_bars(&recorder)[0].clone();
    let bar = pipeline_bar(bar_type, 1_500_000_000);
    data_engine.response(time_range_bar_response(&child, client_id, 1, vec![bar]));

    let received = saver.get_messages();
    assert_eq!(cache.borrow().bar(&bar_type), Some(&bar));
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].correlation_id, parent_id);
    assert_eq!(received[0].bar_type, bar_type);
    assert!(received[0].data.is_empty());
    assert_eq!(
        received[0]
            .params
            .as_ref()
            .and_then(|params| params.get("data_count"))
            .and_then(Value::as_u64),
        Some(1)
    );
}

#[rstest]
fn test_time_range_pipeline_supports_book_deltas_variant(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_test_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(clock.clone(), cache.clone(), None);
    let recorder = register_time_range_recorder(&mut data_engine, clock, cache, client_id, venue);

    let parent_id = UUID4::new();
    let (handler, saver) =
        get_any_saving_handler::<BookDeltasResponse>(Some(Ustr::from("time-range-deltas-parent")));
    msgbus::register_response_handler(&parent_id, handler);
    let live_topic = switchboard::get_book_deltas_topic(instrument_id);
    let pipeline_topic_str = pipeline_topic_of(live_topic.as_ref());
    let pipeline_topic: MStr<Topic> = pipeline_topic_str.as_str().into();
    let (pipeline_handler, pipeline_saver) = get_typed_message_saving_handler::<OrderBookDeltas>(
        Some(Ustr::from("time-range-deltas-payload")),
    );
    msgbus::subscribe_book_deltas(pipeline_topic.into(), pipeline_handler, None);

    let params: Params = serde_json::from_value(json!({"time_range_generator": ""})).unwrap();
    let req = RequestCommand::BookDeltas(RequestBookDeltas::new(
        instrument_id,
        Some(UnixNanos::from(1_000_000_000).to_datetime_utc()),
        Some(UnixNanos::from(2_000_000_000).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        Some(params),
    ));
    data_engine.execute_request(req).unwrap();

    let child = recorded_time_range_request_book_deltas(&recorder)[0].clone();
    let delta = split_delta(instrument_id, 1_500_000_000);
    data_engine.response(time_range_book_deltas_response(
        &child,
        instrument_id,
        client_id,
        1,
        vec![delta],
    ));

    let pipeline_messages = pipeline_saver.get_messages();
    let received = saver.get_messages();
    assert_eq!(pipeline_messages.len(), 1);
    assert_eq!(pipeline_messages[0].instrument_id, instrument_id);
    assert_eq!(pipeline_messages[0].deltas.len(), 1);
    assert_eq!(pipeline_messages[0].deltas[0], delta);
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].correlation_id, parent_id);
    assert_eq!(received[0].instrument_id, instrument_id);
    assert!(received[0].data.is_empty());
    assert_eq!(
        received[0]
            .params
            .as_ref()
            .and_then(|params| params.get("data_count"))
            .and_then(Value::as_u64),
        Some(1)
    );
}

#[rstest]
fn test_time_range_pipeline_supports_book_depth_variant(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_test_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(clock.clone(), cache.clone(), None);
    let recorder = register_time_range_recorder(&mut data_engine, clock, cache, client_id, venue);

    let parent_id = UUID4::new();
    let (handler, saver) =
        get_any_saving_handler::<BookDepthResponse>(Some(Ustr::from("time-range-depth-parent")));
    msgbus::register_response_handler(&parent_id, handler);
    let live_topic = switchboard::get_book_depth_topic(instrument_id);
    let pipeline_topic_str = pipeline_topic_of(live_topic.as_ref());
    let pipeline_topic: MStr<Topic> = pipeline_topic_str.as_str().into();
    let (pipeline_handler, pipeline_saver) = get_typed_message_saving_handler::<OrderBookDepth>(
        Some(Ustr::from("time-range-depth-payload")),
    );
    msgbus::subscribe_book_depth(pipeline_topic.into(), pipeline_handler, None);

    let params: Params = serde_json::from_value(json!({"time_range_generator": ""})).unwrap();
    let depth = NonZeroUsize::new(10).unwrap();
    let req = RequestCommand::BookDepth(RequestBookDepth::new(
        instrument_id,
        Some(UnixNanos::from(1_000_000_000).to_datetime_utc()),
        Some(UnixNanos::from(2_000_000_000).to_datetime_utc()),
        None,
        Some(depth),
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        Some(params),
    ));
    data_engine.execute_request(req).unwrap();

    let child = recorded_time_range_request_book_depth(&recorder)[0].clone();
    assert_eq!(child.depth, Some(depth));
    let depth_msg = book_depth_at(instrument_id, 1_500_000_000);
    data_engine.response(time_range_book_depth_response(
        &child,
        instrument_id,
        client_id,
        1,
        vec![depth_msg.clone()],
    ));

    let pipeline_messages = pipeline_saver.get_messages();
    let received = saver.get_messages();
    assert_eq!(pipeline_messages.len(), 1);
    assert_eq!(pipeline_messages[0], depth_msg);
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].correlation_id, parent_id);
    assert_eq!(received[0].instrument_id, instrument_id);
    assert!(received[0].data.is_empty());
    assert_eq!(
        received[0]
            .params
            .as_ref()
            .and_then(|params| params.get("data_count"))
            .and_then(Value::as_u64),
        Some(1)
    );
}

#[rstest]
fn test_time_range_pipeline_supports_funding_rates_variant(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    advance_test_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(clock.clone(), cache.clone(), None);
    let recorder =
        register_time_range_recorder(&mut data_engine, clock, cache.clone(), client_id, venue);

    let parent_id = UUID4::new();
    let (handler, saver) = get_any_saving_handler::<FundingRatesResponse>(Some(Ustr::from(
        "time-range-funding-parent",
    )));
    msgbus::register_response_handler(&parent_id, handler);

    let params: Params = serde_json::from_value(json!({"time_range_generator": ""})).unwrap();
    let req = RequestCommand::FundingRates(RequestFundingRates::new(
        instrument_id,
        Some(UnixNanos::from(1_000_000_000).to_datetime_utc()),
        Some(UnixNanos::from(2_000_000_000).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        Some(params),
    ));
    data_engine.execute_request(req).unwrap();

    let child = recorded_time_range_request_funding_rates(&recorder)[0].clone();
    let rate = pipeline_funding_rate(instrument_id, 1_500_000_000);
    data_engine.response(time_range_funding_rates_response(
        &child,
        instrument_id,
        client_id,
        1,
        vec![rate],
    ));

    let received = saver.get_messages();
    assert_eq!(cache.borrow().funding_rate(&instrument_id), Some(&rate));
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].correlation_id, parent_id);
    assert_eq!(received[0].instrument_id, instrument_id);
    assert!(received[0].data.is_empty());
    assert_eq!(
        received[0]
            .params
            .as_ref()
            .and_then(|params| params.get("data_count"))
            .and_then(Value::as_u64),
        Some(1)
    );
}

#[rstest]
fn test_pipeline_single_response_passes_through(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache, None);

    let request_id = UUID4::new();
    let (handler, saver) =
        get_any_saving_handler::<QuotesResponse>(Some(Ustr::from("pipeline-single")));
    msgbus::register_response_handler(&request_id, handler);

    data_engine.response(leg_quotes_response(
        request_id,
        instrument_id,
        client_id,
        vec![pipeline_quote(instrument_id, 1_000)],
        None,
        None,
    ));

    let received = saver.get_messages();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].correlation_id, request_id);
    let ts_inits: Vec<u64> = received[0]
        .data
        .iter()
        .map(|q| q.ts_init.as_u64())
        .collect();
    assert_eq!(ts_inits, vec![1_000]);
}

#[rstest]
fn test_pipeline_two_legs_emits_one_rebuilt_response(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache, None);

    let parent_id = UUID4::new();
    let leg_a = UUID4::new();
    let leg_b = UUID4::new();

    let parent_request = RequestCommand::Quotes(RequestQuotes::new(
        instrument_id,
        None,
        None,
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.new_request_pipeline(parent_request, 2);
    data_engine.register_request_pipeline_leg(leg_a, parent_id);
    data_engine.register_request_pipeline_leg(leg_b, parent_id);

    let (handler, saver) =
        get_any_saving_handler::<QuotesResponse>(Some(Ustr::from("pipeline-two")));
    msgbus::register_response_handler(&parent_id, handler);

    data_engine.response(leg_quotes_response(
        leg_a,
        instrument_id,
        client_id,
        vec![pipeline_quote(instrument_id, 2_000)],
        None,
        None,
    ));
    assert!(
        saver.get_messages().is_empty(),
        "parent response must not emit before all legs arrive",
    );

    data_engine.response(leg_quotes_response(
        leg_b,
        instrument_id,
        client_id,
        vec![pipeline_quote(instrument_id, 1_000)],
        None,
        None,
    ));

    let received = saver.get_messages();
    assert_eq!(received.len(), 1);
    let rebuilt = &received[0];
    assert_eq!(rebuilt.correlation_id, parent_id);
    let ts_inits: Vec<u64> = rebuilt.data.iter().map(|q| q.ts_init.as_u64()).collect();
    assert_eq!(ts_inits, vec![1_000, 2_000]);
}

#[rstest]
fn test_pipeline_three_legs_fires_on_third_arrival(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache, None);

    let parent_id = UUID4::new();
    let legs = [UUID4::new(), UUID4::new(), UUID4::new()];

    let parent_request = RequestCommand::Quotes(RequestQuotes::new(
        instrument_id,
        None,
        None,
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.new_request_pipeline(parent_request, legs.len());
    for leg_id in &legs {
        data_engine.register_request_pipeline_leg(*leg_id, parent_id);
    }

    let (handler, saver) =
        get_any_saving_handler::<QuotesResponse>(Some(Ustr::from("pipeline-three")));
    msgbus::register_response_handler(&parent_id, handler);

    for (i, leg_id) in legs.iter().enumerate() {
        data_engine.response(leg_quotes_response(
            *leg_id,
            instrument_id,
            client_id,
            vec![pipeline_quote(instrument_id, (i as u64 + 1) * 1_000)],
            None,
            None,
        ));
    }

    let received = saver.get_messages();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].correlation_id, parent_id);
    assert_eq!(received[0].data.len(), 3);
}

#[rstest]
fn test_pipeline_trims_bounds_on_each_leg(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache, None);

    let parent_id = UUID4::new();
    let leg_a = UUID4::new();
    let leg_b = UUID4::new();

    let parent_request = RequestCommand::Quotes(RequestQuotes::new(
        instrument_id,
        None,
        None,
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.new_request_pipeline(parent_request, 2);
    data_engine.register_request_pipeline_leg(leg_a, parent_id);
    data_engine.register_request_pipeline_leg(leg_b, parent_id);

    let (handler, saver) =
        get_any_saving_handler::<QuotesResponse>(Some(Ustr::from("pipeline-trim")));
    msgbus::register_response_handler(&parent_id, handler);

    data_engine.response(leg_quotes_response(
        leg_a,
        instrument_id,
        client_id,
        vec![
            pipeline_quote(instrument_id, 1_000),
            pipeline_quote(instrument_id, 2_000),
            pipeline_quote(instrument_id, 3_000),
        ],
        None,
        Some(UnixNanos::from(2_000)),
    ));
    data_engine.response(leg_quotes_response(
        leg_b,
        instrument_id,
        client_id,
        vec![
            pipeline_quote(instrument_id, 4_000),
            pipeline_quote(instrument_id, 5_000),
            pipeline_quote(instrument_id, 6_000),
        ],
        Some(UnixNanos::from(5_000)),
        None,
    ));

    let received = saver.get_messages();
    assert_eq!(received.len(), 1);
    let ts_inits: Vec<u64> = received[0]
        .data
        .iter()
        .map(|q| q.ts_init.as_u64())
        .collect();
    assert_eq!(ts_inits, vec![1_000, 2_000, 5_000, 6_000]);
}

#[rstest]
fn test_request_join_two_phase_emits_parent_response(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    // Advance the test clock past the leg ts_init values so the join's
    // `_bound_dates` clamping does not collapse the parent window to 0.
    clock
        .borrow_mut()
        .as_any_mut()
        .downcast_mut::<VirtualClock>()
        .unwrap()
        .advance_time(UnixNanos::from(10_000_000_000_u64), true);
    let mut data_engine = DataEngine::new(clock, cache.clone(), None);

    let leg_a = UUID4::new();
    let leg_b = UUID4::new();
    let join_id = UUID4::new();

    let join = RequestJoin::new(
        vec![leg_a, leg_b],
        None,
        None,
        join_id,
        UnixNanos::default(),
        None,
        None,
    );
    data_engine
        .execute_request(RequestCommand::Join(join))
        .unwrap();

    let (parent_handler, parent_saver) =
        get_any_saving_handler::<QuotesResponse>(Some(Ustr::from("join-parent")));
    msgbus::register_response_handler(&join_id, parent_handler);
    let (leg_a_handler, leg_a_saver) =
        get_any_saving_handler::<QuotesResponse>(Some(Ustr::from("join-leg-a")));
    msgbus::register_response_handler(&leg_a, leg_a_handler);
    let (leg_b_handler, leg_b_saver) =
        get_any_saving_handler::<QuotesResponse>(Some(Ustr::from("join-leg-b")));
    msgbus::register_response_handler(&leg_b, leg_b_handler);

    data_engine.response(leg_quotes_response(
        leg_a,
        instrument_id,
        client_id,
        vec![pipeline_quote(instrument_id, 1_000)],
        None,
        None,
    ));
    data_engine.response(leg_quotes_response(
        leg_b,
        instrument_id,
        client_id,
        vec![pipeline_quote(instrument_id, 2_000)],
        None,
        None,
    ));

    let parent = parent_saver.get_messages();
    assert_eq!(parent.len(), 1, "expected one final join response");
    assert_eq!(parent[0].correlation_id, join_id);
    let ts_inits: Vec<u64> = parent[0].data.iter().map(|q| q.ts_init.as_u64()).collect();
    assert_eq!(ts_inits, vec![1_000, 2_000]);

    assert_eq!(leg_a_saver.get_messages().len(), 1);
    assert!(leg_a_saver.get_messages()[0].data.is_empty());
    assert_eq!(leg_b_saver.get_messages().len(), 1);
    assert!(leg_b_saver.get_messages()[0].data.is_empty());

    // Joined data must reach the cache via the normal per-variant handler
    // path; the final response routes through `response()` after the
    // pipeline + join gates are cleared. The cache keeps the latest quote,
    // so we expect the leg with the higher ts_init.
    let cached = cache
        .borrow()
        .quote(&instrument_id)
        .copied()
        .expect("joined quote data must reach the cache");
    assert_eq!(cached.ts_init, UnixNanos::from(2_000));

    for request_id in [leg_a, leg_b, join_id] {
        assert!(
            stub_msgbus
                .borrow()
                .get_response_handler(&request_id)
                .is_none(),
            "completed join response handler must be removed for {request_id}",
        );
    }

    data_engine.response(leg_quotes_response(
        leg_a,
        instrument_id,
        client_id,
        vec![pipeline_quote(instrument_id, 3_000)],
        None,
        None,
    ));
    data_engine.response(leg_quotes_response(
        leg_b,
        instrument_id,
        client_id,
        vec![pipeline_quote(instrument_id, 4_000)],
        None,
        None,
    ));
    data_engine.response(leg_quotes_response(
        join_id,
        instrument_id,
        client_id,
        vec![pipeline_quote(instrument_id, 5_000)],
        None,
        None,
    ));

    assert_eq!(
        parent_saver.get_messages().len(),
        1,
        "late leg and parent responses must not complete the join again",
    );
    assert_eq!(leg_a_saver.get_messages().len(), 1);
    assert_eq!(leg_b_saver.get_messages().len(), 1);
}

#[rstest]
fn test_request_join_trims_to_parent_window(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    clock
        .borrow_mut()
        .as_any_mut()
        .downcast_mut::<VirtualClock>()
        .unwrap()
        .advance_time(UnixNanos::from(10_000_000_000_u64), true);
    let mut data_engine = DataEngine::new(clock.clone(), cache, None);

    let leg_a = UUID4::new();
    let leg_b = UUID4::new();
    let join_id = UUID4::new();

    let join_start = UnixNanos::from(2_000).to_datetime_utc();
    let join_end = UnixNanos::from(4_000).to_datetime_utc();

    let join = RequestJoin::new(
        vec![leg_a, leg_b],
        Some(join_start),
        Some(join_end),
        join_id,
        UnixNanos::default(),
        None,
        None,
    );
    data_engine
        .execute_request(RequestCommand::Join(join))
        .unwrap();

    let (parent_handler, parent_saver) =
        get_any_saving_handler::<QuotesResponse>(Some(Ustr::from("join-window")));
    msgbus::register_response_handler(&join_id, parent_handler);

    // Legs return wider data than the parent join window; only entries in
    // `[2_000, 4_000]` should survive the final response.
    data_engine.response(leg_quotes_response(
        leg_a,
        instrument_id,
        client_id,
        vec![
            pipeline_quote(instrument_id, 1_000),
            pipeline_quote(instrument_id, 2_500),
        ],
        None,
        None,
    ));
    data_engine.response(leg_quotes_response(
        leg_b,
        instrument_id,
        client_id,
        vec![
            pipeline_quote(instrument_id, 3_500),
            pipeline_quote(instrument_id, 5_000),
        ],
        None,
        None,
    ));

    let parent = parent_saver.get_messages();
    assert_eq!(parent.len(), 1);
    let ts_inits: Vec<u64> = parent[0].data.iter().map(|q| q.ts_init.as_u64()).collect();
    assert_eq!(ts_inits, vec![2_500, 3_500]);
}

#[rstest]
fn test_pipeline_two_legs_trims_against_parent_window(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache, None);

    let parent_id = UUID4::new();
    let leg_a = UUID4::new();
    let leg_b = UUID4::new();

    let parent_request = RequestCommand::Quotes(RequestQuotes::new(
        instrument_id,
        Some(UnixNanos::from(2_000).to_datetime_utc()),
        Some(UnixNanos::from(4_000).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.new_request_pipeline(parent_request, 2);
    data_engine.register_request_pipeline_leg(leg_a, parent_id);
    data_engine.register_request_pipeline_leg(leg_b, parent_id);

    let (handler, saver) =
        get_any_saving_handler::<QuotesResponse>(Some(Ustr::from("pipeline-parent-window-trim")));
    msgbus::register_response_handler(&parent_id, handler);

    data_engine.response(leg_quotes_response(
        leg_a,
        instrument_id,
        client_id,
        vec![
            pipeline_quote(instrument_id, 1_000),
            pipeline_quote(instrument_id, 2_500),
        ],
        None,
        None,
    ));
    data_engine.response(leg_quotes_response(
        leg_b,
        instrument_id,
        client_id,
        vec![
            pipeline_quote(instrument_id, 3_500),
            pipeline_quote(instrument_id, 5_000),
        ],
        None,
        None,
    ));

    let received = saver.get_messages();
    assert_eq!(received.len(), 1);
    let ts_inits: Vec<u64> = received[0]
        .data
        .iter()
        .map(|q| q.ts_init.as_u64())
        .collect();
    assert_eq!(ts_inits, vec![2_500, 3_500]);
}

#[rstest]
fn test_pipeline_two_legs_inherits_parent_bars_window(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let bar_type = BarType::from(format!("{}-1-MINUTE-LAST-EXTERNAL", audusd_sim.id).as_str());
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache, None);

    let parent_id = UUID4::new();
    let leg_a = UUID4::new();
    let leg_b = UUID4::new();

    let parent_request = RequestCommand::Bars(RequestBars::new(
        bar_type,
        Some(UnixNanos::from(2_000).to_datetime_utc()),
        Some(UnixNanos::from(4_000).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.new_request_pipeline(parent_request, 2);
    data_engine.register_request_pipeline_leg(leg_a, parent_id);
    data_engine.register_request_pipeline_leg(leg_b, parent_id);

    let (handler, saver) =
        get_any_saving_handler::<BarsResponse>(Some(Ustr::from("pipeline-parent-bars-window")));
    msgbus::register_response_handler(&parent_id, handler);

    data_engine.response(leg_bars_response(
        leg_a,
        bar_type,
        client_id,
        vec![pipeline_bar(bar_type, 1_000), pipeline_bar(bar_type, 2_500)],
        None,
        None,
    ));
    data_engine.response(leg_bars_response(
        leg_b,
        bar_type,
        client_id,
        vec![pipeline_bar(bar_type, 3_500), pipeline_bar(bar_type, 5_000)],
        None,
        None,
    ));

    let received = saver.get_messages();
    assert_eq!(received.len(), 1);
    let ts_inits: Vec<u64> = received[0]
        .data
        .iter()
        .map(|b| b.ts_init.as_u64())
        .collect();
    assert_eq!(ts_inits, vec![2_500, 3_500]);
}

#[rstest]
fn test_pipeline_two_legs_with_no_parent_window_preserves_leg_bounds(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache, None);

    let parent_id = UUID4::new();
    let leg_a = UUID4::new();
    let leg_b = UUID4::new();

    let parent_request = RequestCommand::Quotes(RequestQuotes::new(
        instrument_id,
        None,
        None,
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.new_request_pipeline(parent_request, 2);
    data_engine.register_request_pipeline_leg(leg_a, parent_id);
    data_engine.register_request_pipeline_leg(leg_b, parent_id);

    let (handler, saver) =
        get_any_saving_handler::<QuotesResponse>(Some(Ustr::from("pipeline-no-parent-window")));
    msgbus::register_response_handler(&parent_id, handler);

    data_engine.response(leg_quotes_response(
        leg_a,
        instrument_id,
        client_id,
        vec![pipeline_quote(instrument_id, 1_500)],
        Some(UnixNanos::from(1_000)),
        Some(UnixNanos::from(2_000)),
    ));
    data_engine.response(leg_quotes_response(
        leg_b,
        instrument_id,
        client_id,
        vec![pipeline_quote(instrument_id, 3_500)],
        Some(UnixNanos::from(3_000)),
        Some(UnixNanos::from(4_000)),
    ));

    let received = saver.get_messages();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].start, Some(UnixNanos::from(1_000)));
    assert_eq!(received[0].end, Some(UnixNanos::from(2_000)));
    let ts_inits: Vec<u64> = received[0]
        .data
        .iter()
        .map(|q| q.ts_init.as_u64())
        .collect();
    assert_eq!(ts_inits, vec![1_500, 3_500]);
}

#[rstest]
fn test_pipeline_trims_when_only_parent_start_is_set(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache, None);

    let parent_id = UUID4::new();
    let leg_a = UUID4::new();
    let leg_b = UUID4::new();

    let parent_request = RequestCommand::Quotes(RequestQuotes::new(
        instrument_id,
        Some(UnixNanos::from(2_000).to_datetime_utc()),
        None,
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.new_request_pipeline(parent_request, 2);
    data_engine.register_request_pipeline_leg(leg_a, parent_id);
    data_engine.register_request_pipeline_leg(leg_b, parent_id);

    let (handler, saver) =
        get_any_saving_handler::<QuotesResponse>(Some(Ustr::from("pipeline-trim-start-only")));
    msgbus::register_response_handler(&parent_id, handler);

    data_engine.response(leg_quotes_response(
        leg_a,
        instrument_id,
        client_id,
        vec![
            pipeline_quote(instrument_id, 1_000),
            pipeline_quote(instrument_id, 2_500),
        ],
        None,
        None,
    ));
    data_engine.response(leg_quotes_response(
        leg_b,
        instrument_id,
        client_id,
        vec![pipeline_quote(instrument_id, 3_500)],
        None,
        None,
    ));

    let received = saver.get_messages();
    assert_eq!(received.len(), 1);
    let ts_inits: Vec<u64> = received[0]
        .data
        .iter()
        .map(|q| q.ts_init.as_u64())
        .collect();
    assert_eq!(
        ts_inits,
        vec![2_500, 3_500],
        "start-only parent window drops only the pre-start entries"
    );
}

#[rstest]
fn test_pipeline_trims_when_only_parent_end_is_set(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache, None);

    let parent_id = UUID4::new();
    let leg_a = UUID4::new();
    let leg_b = UUID4::new();

    let parent_request = RequestCommand::Quotes(RequestQuotes::new(
        instrument_id,
        None,
        Some(UnixNanos::from(2_500).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.new_request_pipeline(parent_request, 2);
    data_engine.register_request_pipeline_leg(leg_a, parent_id);
    data_engine.register_request_pipeline_leg(leg_b, parent_id);

    let (handler, saver) =
        get_any_saving_handler::<QuotesResponse>(Some(Ustr::from("pipeline-trim-end-only")));
    msgbus::register_response_handler(&parent_id, handler);

    data_engine.response(leg_quotes_response(
        leg_a,
        instrument_id,
        client_id,
        vec![pipeline_quote(instrument_id, 1_000)],
        None,
        None,
    ));
    data_engine.response(leg_quotes_response(
        leg_b,
        instrument_id,
        client_id,
        vec![
            pipeline_quote(instrument_id, 2_500),
            pipeline_quote(instrument_id, 3_500),
        ],
        None,
        None,
    ));

    let received = saver.get_messages();
    assert_eq!(received.len(), 1);
    let ts_inits: Vec<u64> = received[0]
        .data
        .iter()
        .map(|q| q.ts_init.as_u64())
        .collect();
    assert_eq!(
        ts_inits,
        vec![1_000, 2_500],
        "end-only parent window drops only the post-end entries"
    );
}

#[rstest]
fn test_reset_clears_pipeline_and_join_state(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache, None);

    let leg_a = UUID4::new();
    let leg_b = UUID4::new();
    let join_id = UUID4::new();

    let join = RequestJoin::new(
        vec![leg_a, leg_b],
        None,
        None,
        join_id,
        UnixNanos::default(),
        None,
        None,
    );
    data_engine
        .execute_request(RequestCommand::Join(join))
        .unwrap();

    assert_eq!(data_engine.request_pipeline_count(), 1);
    assert_eq!(data_engine.pending_join_request_count(), 1);

    let (parent_handler, parent_saver) =
        get_any_saving_handler::<QuotesResponse>(Some(Ustr::from("reset-parent")));
    msgbus::register_response_handler(&join_id, parent_handler);
    let (leg_b_handler, leg_b_saver) =
        get_any_saving_handler::<QuotesResponse>(Some(Ustr::from("reset-leg-b")));
    msgbus::register_response_handler(&leg_b, leg_b_handler);

    data_engine.reset();

    assert_eq!(data_engine.request_pipeline_count(), 0);
    assert_eq!(data_engine.pending_join_request_count(), 0);

    for request_id in [leg_b, join_id] {
        assert!(
            stub_msgbus
                .borrow()
                .get_response_handler(&request_id)
                .is_some(),
            "data engine reset must not clear message bus response handlers for {request_id}",
        );
    }

    data_engine.response(leg_quotes_response(
        leg_a,
        instrument_id,
        client_id,
        vec![pipeline_quote(instrument_id, 1_000)],
        None,
        None,
    ));
    data_engine.response(leg_quotes_response(
        leg_b,
        instrument_id,
        client_id,
        vec![pipeline_quote(instrument_id, 2_000)],
        None,
        None,
    ));

    assert_eq!(leg_b_saver.get_messages().len(), 1);
    assert_eq!(leg_b_saver.get_messages()[0].correlation_id, leg_b);
    assert!(
        stub_msgbus.borrow().get_response_handler(&leg_b).is_none(),
        "the first late leg response must consume its handler",
    );

    data_engine.response(leg_quotes_response(
        leg_b,
        instrument_id,
        client_id,
        vec![pipeline_quote(instrument_id, 3_000)],
        None,
        None,
    ));

    assert_eq!(
        leg_b_saver.get_messages().len(),
        1,
        "a duplicate late leg response must not invoke the handler again",
    );
    assert!(
        parent_saver.get_messages().is_empty(),
        "reset must clear pipeline state so no rebuilt parent fires",
    );
    assert!(
        stub_msgbus
            .borrow()
            .get_response_handler(&join_id)
            .is_some(),
        "reset must not clear unrelated message bus response handlers",
    );
}

#[rstest]
fn test_pipeline_unsupported_variant_drops_response(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = stub_msgbus;
    let _ = audusd_sim;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache, None);

    let parent_id = UUID4::new();
    let leg_id = UUID4::new();

    let series_id = OptionSeriesId::new(
        venue,
        Ustr::from("ES"),
        Ustr::from("USD"),
        UnixNanos::from(100),
    );
    let parent_request =
        RequestCommand::OptionChainReferencePrice(RequestOptionChainReferencePrice::new(
            series_id,
            InstrumentId::from("ES-TEST-5000-C.SIM"),
            Some(client_id),
            parent_id,
            UnixNanos::default(),
            None,
        ));
    data_engine.new_request_pipeline(parent_request, 1);
    data_engine.register_request_pipeline_leg(leg_id, parent_id);

    let (parent_handler, parent_saver) = get_any_saving_handler::<OptionChainReferencePriceResponse>(
        Some(Ustr::from("pipeline-unsupported-parent")),
    );
    msgbus::register_response_handler(&parent_id, parent_handler);
    let (leg_handler, leg_saver) = get_any_saving_handler::<OptionChainReferencePriceResponse>(
        Some(Ustr::from("pipeline-unsupported-leg")),
    );
    msgbus::register_response_handler(&leg_id, leg_handler);

    data_engine.response(DataResponse::OptionChainReferencePrice(
        OptionChainReferencePriceResponse::new(
            leg_id,
            client_id,
            series_id,
            None,
            UnixNanos::default(),
            None,
        ),
    ));

    assert!(
        parent_saver.get_messages().is_empty(),
        "unsupported pipeline variant must not emit a parent-keyed response",
    );
    assert!(
        leg_saver.get_messages().is_empty(),
        "unsupported pipeline variant must not leak the leg response unchanged",
    );
    assert_eq!(data_engine.request_pipeline_count(), 0);
}

#[rstest]
fn test_request_join_new_panics_on_empty_request_ids() {
    let result = std::panic::catch_unwind(|| {
        RequestJoin::new(
            Vec::new(),
            None,
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        )
    });

    let err = result.expect_err("RequestJoin::new must panic on empty request_ids");
    let msg = err
        .downcast_ref::<&'static str>()
        .map(|s| (*s).to_string())
        .or_else(|| err.downcast_ref::<String>().cloned())
        .unwrap_or_default();
    assert!(
        msg.contains("request_ids must not be empty"),
        "unexpected panic message: {msg}",
    );
}

#[rstest]
fn test_request_join_with_dates_inherits_originals() {
    let original_id = UUID4::new();
    let request_ids = vec![UUID4::new(), UUID4::new()];
    let params: Params = serde_json::from_value(json!({"flag": "value"})).unwrap();

    let original = RequestJoin::new(
        request_ids.clone(),
        None,
        None,
        original_id,
        UnixNanos::default(),
        Some(params.clone()),
        None,
    );

    let new_start = UnixNanos::from(1_000).to_datetime_utc();
    let new_end = UnixNanos::from(5_000).to_datetime_utc();
    let dated = original.with_dates(Some(new_start), Some(new_end), UnixNanos::from(42));

    assert_eq!(dated.request_ids, request_ids);
    assert_eq!(dated.start, Some(new_start));
    assert_eq!(dated.end, Some(new_end));
    assert_eq!(dated.ts_init, UnixNanos::from(42));
    assert_eq!(dated.correlation_id, Some(original_id));
    assert_ne!(dated.request_id, original_id);
    assert_eq!(dated.params, Some(params));
}

#[rstest]
fn test_pipeline_reset_mid_buffer_clears_partial_state(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache, None);

    let parent_id = UUID4::new();
    let leg_a = UUID4::new();
    let leg_b = UUID4::new();

    let parent_request = RequestCommand::Quotes(RequestQuotes::new(
        instrument_id,
        None,
        None,
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.new_request_pipeline(parent_request, 2);
    data_engine.register_request_pipeline_leg(leg_a, parent_id);
    data_engine.register_request_pipeline_leg(leg_b, parent_id);

    let (parent_handler, parent_saver) =
        get_any_saving_handler::<QuotesResponse>(Some(Ustr::from("mid-reset-parent")));
    msgbus::register_response_handler(&parent_id, parent_handler);
    let (leg_b_handler, leg_b_saver) =
        get_any_saving_handler::<QuotesResponse>(Some(Ustr::from("mid-reset-leg-b")));
    msgbus::register_response_handler(&leg_b, leg_b_handler);

    data_engine.response(leg_quotes_response(
        leg_a,
        instrument_id,
        client_id,
        vec![pipeline_quote(instrument_id, 1_000)],
        None,
        None,
    ));
    assert_eq!(data_engine.request_pipeline_count(), 1);

    data_engine.reset();
    assert_eq!(data_engine.request_pipeline_count(), 0);

    // After reset, the second leg is no longer registered with any pipeline,
    // so it must propagate to msgbus under its own correlation_id.
    data_engine.response(leg_quotes_response(
        leg_b,
        instrument_id,
        client_id,
        vec![pipeline_quote(instrument_id, 2_000)],
        None,
        None,
    ));

    assert!(
        parent_saver.get_messages().is_empty(),
        "no rebuilt parent must fire after mid-buffer reset",
    );
    assert_eq!(leg_b_saver.get_messages().len(), 1);
    assert_eq!(leg_b_saver.get_messages()[0].correlation_id, leg_b);
}

#[rstest]
fn test_request_join_single_leg_fires_immediately(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    clock
        .borrow_mut()
        .as_any_mut()
        .downcast_mut::<VirtualClock>()
        .unwrap()
        .advance_time(UnixNanos::from(10_000_000_000_u64), true);
    let mut data_engine = DataEngine::new(clock, cache, None);

    let leg = UUID4::new();
    let join_id = UUID4::new();

    let join = RequestJoin::new(
        vec![leg],
        None,
        None,
        join_id,
        UnixNanos::default(),
        None,
        None,
    );
    data_engine
        .execute_request(RequestCommand::Join(join))
        .unwrap();

    let (parent_handler, parent_saver) =
        get_any_saving_handler::<QuotesResponse>(Some(Ustr::from("single-leg-parent")));
    msgbus::register_response_handler(&join_id, parent_handler);
    let (leg_handler, leg_saver) =
        get_any_saving_handler::<QuotesResponse>(Some(Ustr::from("single-leg-leg")));
    msgbus::register_response_handler(&leg, leg_handler);

    data_engine.response(leg_quotes_response(
        leg,
        instrument_id,
        client_id,
        vec![pipeline_quote(instrument_id, 1_000)],
        None,
        None,
    ));

    let parent = parent_saver.get_messages();
    assert_eq!(parent.len(), 1);
    assert_eq!(parent[0].correlation_id, join_id);
    let ts_inits: Vec<u64> = parent[0].data.iter().map(|q| q.ts_init.as_u64()).collect();
    assert_eq!(ts_inits, vec![1_000]);

    assert_eq!(leg_saver.get_messages().len(), 1);
    assert!(leg_saver.get_messages()[0].data.is_empty());

    assert_eq!(data_engine.request_pipeline_count(), 0);
    assert_eq!(data_engine.pending_join_request_count(), 0);

    for request_id in [leg, join_id] {
        assert!(
            stub_msgbus
                .borrow()
                .get_response_handler(&request_id)
                .is_none(),
            "completed single-leg join handler must be removed for {request_id}",
        );
    }
}

#[rstest]
fn test_request_join_rebuilds_same_instrument_book_deltas_legs(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    // Past the leg ts_init values, so the join's bound-date clamping does not
    // collapse the parent window to 0.
    advance_test_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(clock, cache, None);

    let leg_a = UUID4::new();
    let leg_b = UUID4::new();
    let join_id = UUID4::new();

    data_engine
        .execute_request(RequestCommand::Join(RequestJoin::new(
            vec![leg_a, leg_b],
            None,
            None,
            join_id,
            UnixNanos::default(),
            None,
            None,
        )))
        .unwrap();

    let (parent_handler, parent_saver) = get_any_saving_handler::<BookDeltasResponse>(Some(
        Ustr::from("same-instrument-deltas-parent"),
    ));
    msgbus::register_response_handler(&join_id, parent_handler);

    data_engine.response(leg_book_deltas_response(
        leg_a,
        instrument_id,
        client_id,
        vec![delta_with_flag(
            instrument_id,
            1_000,
            RecordFlag::F_LAST as u8,
        )],
    ));
    data_engine.response(leg_book_deltas_response(
        leg_b,
        instrument_id,
        client_id,
        vec![delta_with_flag(
            instrument_id,
            2_000,
            RecordFlag::F_LAST as u8,
        )],
    ));

    let responses = parent_saver.get_messages();
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0].instrument_id, instrument_id);
    assert_eq!(
        responses[0]
            .data
            .iter()
            .map(|delta| delta.ts_init.as_u64())
            .collect::<Vec<_>>(),
        vec![1_000, 2_000],
    );
    assert_eq!(data_engine.pending_join_request_count(), 0);
}

#[rstest]
fn test_request_join_mixed_instrument_book_deltas_cleans_up_join_staging(
    audusd_sim: CurrencyPair,
    gbpusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    // Past the leg ts_init values, so the deltas survive the parent-window trim and
    // reach the response handler when the rebuild is not refused.
    advance_test_clock_to(&clock, 10_000_000_000);
    let mut data_engine = DataEngine::new(clock, cache, None);

    let leg_a = UUID4::new();
    let leg_b = UUID4::new();
    let join_id = UUID4::new();

    data_engine
        .execute_request(RequestCommand::Join(RequestJoin::new(
            vec![leg_a, leg_b],
            None,
            None,
            join_id,
            UnixNanos::default(),
            None,
            None,
        )))
        .unwrap();

    let (parent_handler, parent_saver) = get_any_saving_handler::<BookDeltasResponse>(Some(
        Ustr::from("mixed-instrument-deltas-parent"),
    ));
    msgbus::register_response_handler(&join_id, parent_handler);

    data_engine.response(leg_book_deltas_response(
        leg_a,
        audusd_sim.id,
        client_id,
        vec![delta_with_flag(
            audusd_sim.id,
            1_000,
            RecordFlag::F_LAST as u8,
        )],
    ));
    data_engine.response(leg_book_deltas_response(
        leg_b,
        gbpusd_sim.id,
        client_id,
        vec![delta_with_flag(
            gbpusd_sim.id,
            2_000,
            RecordFlag::F_LAST as u8,
        )],
    ));

    assert!(
        parent_saver.get_messages().is_empty(),
        "mixed-instrument rebuild must not emit a parent response",
    );
    assert_eq!(
        data_engine.request_pipeline_count(),
        0,
        "pipeline state must be cleared after a failed rebuild",
    );
    assert_eq!(
        data_engine.pending_join_request_count(),
        0,
        "pending join must be cleared after a failed rebuild to prevent leaks",
    );
}

#[rstest]
fn test_request_join_mixed_variants_cleans_up_join_staging(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache, None);

    let leg_a = UUID4::new();
    let leg_b = UUID4::new();
    let join_id = UUID4::new();

    let join = RequestJoin::new(
        vec![leg_a, leg_b],
        None,
        None,
        join_id,
        UnixNanos::default(),
        None,
        None,
    );
    data_engine
        .execute_request(RequestCommand::Join(join))
        .unwrap();

    let (parent_handler, parent_saver) =
        get_any_saving_handler::<QuotesResponse>(Some(Ustr::from("mixed-variant-parent")));
    msgbus::register_response_handler(&join_id, parent_handler);

    data_engine.response(leg_quotes_response(
        leg_a,
        instrument_id,
        client_id,
        vec![pipeline_quote(instrument_id, 1_000)],
        None,
        None,
    ));

    let make_trade = |ts: u64| {
        TradeTick::new(
            instrument_id,
            Price::from("1.00000"),
            Quantity::from("1"),
            AggressorSide::Buy,
            TradeId::new(format!("t-{ts}")),
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };

    data_engine.response(DataResponse::Trades(TradesResponse::new(
        leg_b,
        client_id,
        instrument_id,
        vec![make_trade(2_000)],
        None,
        None,
        UnixNanos::default(),
        None,
    )));

    assert!(
        parent_saver.get_messages().is_empty(),
        "mixed-variant rebuild must not emit a parent response",
    );
    assert_eq!(
        data_engine.request_pipeline_count(),
        0,
        "pipeline state must be cleared after a failed rebuild",
    );
    assert_eq!(
        data_engine.pending_join_request_count(),
        0,
        "pending join must be cleared after a failed rebuild to prevent leaks",
    );
}

#[rstest]
fn test_pipeline_one_empty_leg_still_emits_parent(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache, None);

    let parent_id = UUID4::new();
    let leg_a = UUID4::new();
    let leg_b = UUID4::new();

    let parent_request = RequestCommand::Quotes(RequestQuotes::new(
        instrument_id,
        None,
        None,
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    data_engine.new_request_pipeline(parent_request, 2);
    data_engine.register_request_pipeline_leg(leg_a, parent_id);
    data_engine.register_request_pipeline_leg(leg_b, parent_id);

    let (parent_handler, parent_saver) =
        get_any_saving_handler::<QuotesResponse>(Some(Ustr::from("empty-leg-parent")));
    msgbus::register_response_handler(&parent_id, parent_handler);

    data_engine.response(leg_quotes_response(
        leg_a,
        instrument_id,
        client_id,
        vec![
            pipeline_quote(instrument_id, 1_000),
            pipeline_quote(instrument_id, 2_000),
        ],
        None,
        None,
    ));
    data_engine.response(leg_quotes_response(
        leg_b,
        instrument_id,
        client_id,
        Vec::new(),
        None,
        None,
    ));

    let received = parent_saver.get_messages();
    assert_eq!(received.len(), 1);
    let ts_inits: Vec<u64> = received[0]
        .data
        .iter()
        .map(|q| q.ts_init.as_u64())
        .collect();
    assert_eq!(ts_inits, vec![1_000, 2_000]);
}

#[rstest]
fn test_request_join_all_empty_legs_emits_empty_parent(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
) {
    let _ = stub_msgbus;
    let instrument_id = audusd_sim.id;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let mut data_engine = DataEngine::new(clock, cache, None);

    let leg_a = UUID4::new();
    let leg_b = UUID4::new();
    let join_id = UUID4::new();

    let join = RequestJoin::new(
        vec![leg_a, leg_b],
        None,
        None,
        join_id,
        UnixNanos::default(),
        None,
        None,
    );
    data_engine
        .execute_request(RequestCommand::Join(join))
        .unwrap();

    let (parent_handler, parent_saver) =
        get_any_saving_handler::<QuotesResponse>(Some(Ustr::from("all-empty-parent")));
    msgbus::register_response_handler(&join_id, parent_handler);

    data_engine.response(leg_quotes_response(
        leg_a,
        instrument_id,
        client_id,
        Vec::new(),
        None,
        None,
    ));
    data_engine.response(leg_quotes_response(
        leg_b,
        instrument_id,
        client_id,
        Vec::new(),
        None,
        None,
    ));

    let received = parent_saver.get_messages();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].correlation_id, join_id);
    assert!(received[0].data.is_empty());
    assert_eq!(data_engine.pending_join_request_count(), 0);
}

#[cfg(feature = "streaming")]
#[rstest]
fn test_request_quotes_dispatch_failure_aborts_pipeline(
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
    let mut data_engine = DataEngine::new(clock, cache, None);

    let _catalog_dir = register_quote_catalog_with_quotes(
        &mut data_engine,
        "abort-pipeline",
        &[split_quote(instrument_id, 1_500)],
        Some((1_000, 1_500)),
    );

    let failing = FailingRequestDataClient::new(client_id, Some(venue), "client refused");
    let adapter = DataClientAdapter::new(client_id, Some(venue), true, true, Box::new(failing));
    data_engine.register_client(adapter, None);

    let parent_id = UUID4::new();
    let (handler, saver) =
        get_any_saving_handler::<QuotesResponse>(Some(Ustr::from("abort-pipeline-parent")));
    msgbus::register_response_handler(&parent_id, handler);

    let req = RequestCommand::Quotes(RequestQuotes::new(
        instrument_id,
        Some(UnixNanos::from(1_000).to_datetime_utc()),
        Some(UnixNanos::from(3_000).to_datetime_utc()),
        None,
        Some(client_id),
        parent_id,
        UnixNanos::default(),
        None,
    ));
    let err = data_engine
        .execute_request(req)
        .expect_err("client leg dispatch failure must propagate to the caller");
    let err_message = format!("{err:#}");
    assert!(
        err_message.contains("client refused"),
        "error must originate in the failing client (was: {err_message})"
    );
    assert_eq!(
        data_engine.request_pipeline_count(),
        0,
        "abort_request_pipeline must drain pipeline state on dispatch failure"
    );
    assert!(
        saver.get_messages().is_empty(),
        "no rebuilt response must reach the parent handler when dispatch fails"
    );
}

fn pipeline_topic_of(live: &str) -> String {
    let suffix = live.strip_prefix("data.").unwrap_or(live);
    format!("data.pipeline.{suffix}")
}

fn time_range_trade_response(
    request: &RequestTrades,
    instrument_id: InstrumentId,
    client_id: ClientId,
    data_count: u64,
    trades: Vec<TradeTick>,
) -> DataResponse {
    DataResponse::Trades(TradesResponse::new(
        request.request_id,
        client_id,
        instrument_id,
        trades,
        request.start.map(datetime_to_unix_nanos_for_test),
        request.end.map(datetime_to_unix_nanos_for_test),
        UnixNanos::default(),
        Some(time_range_data_count_params(data_count)),
    ))
}

fn time_range_bar_response(
    request: &RequestBars,
    client_id: ClientId,
    data_count: u64,
    bars: Vec<Bar>,
) -> DataResponse {
    DataResponse::Bars(BarsResponse::new(
        request.request_id,
        client_id,
        request.bar_type,
        bars,
        request.start.map(datetime_to_unix_nanos_for_test),
        request.end.map(datetime_to_unix_nanos_for_test),
        UnixNanos::default(),
        Some(time_range_data_count_params(data_count)),
    ))
}

fn time_range_book_deltas_response(
    request: &RequestBookDeltas,
    instrument_id: InstrumentId,
    client_id: ClientId,
    data_count: u64,
    deltas: Vec<OrderBookDelta>,
) -> DataResponse {
    DataResponse::BookDeltas(BookDeltasResponse::new(
        request.request_id,
        client_id,
        instrument_id,
        deltas,
        request.start.map(datetime_to_unix_nanos_for_test),
        request.end.map(datetime_to_unix_nanos_for_test),
        UnixNanos::default(),
        Some(time_range_data_count_params(data_count)),
    ))
}

fn time_range_book_depth_response(
    request: &RequestBookDepth,
    instrument_id: InstrumentId,
    client_id: ClientId,
    data_count: u64,
    depths: Vec<OrderBookDepth>,
) -> DataResponse {
    DataResponse::BookDepth(BookDepthResponse::new(
        request.request_id,
        client_id,
        instrument_id,
        depths,
        request.start.map(datetime_to_unix_nanos_for_test),
        request.end.map(datetime_to_unix_nanos_for_test),
        UnixNanos::default(),
        Some(time_range_data_count_params(data_count)),
    ))
}

fn time_range_funding_rates_response(
    request: &RequestFundingRates,
    instrument_id: InstrumentId,
    client_id: ClientId,
    data_count: u64,
    rates: Vec<FundingRateUpdate>,
) -> DataResponse {
    DataResponse::FundingRates(FundingRatesResponse::new(
        request.request_id,
        client_id,
        instrument_id,
        rates,
        request.start.map(datetime_to_unix_nanos_for_test),
        request.end.map(datetime_to_unix_nanos_for_test),
        UnixNanos::default(),
        Some(time_range_data_count_params(data_count)),
    ))
}

fn recorded_time_range_request_quotes(
    recorder: &Rc<RefCell<Vec<DataCommand>>>,
) -> Vec<RequestQuotes> {
    recorder
        .borrow()
        .iter()
        .filter_map(|cmd| match cmd {
            DataCommand::Request(RequestCommand::Quotes(req)) => Some(req.clone()),
            _ => None,
        })
        .collect()
}

fn recorded_time_range_request_trades(
    recorder: &Rc<RefCell<Vec<DataCommand>>>,
) -> Vec<RequestTrades> {
    recorder
        .borrow()
        .iter()
        .filter_map(|cmd| match cmd {
            DataCommand::Request(RequestCommand::Trades(req)) => Some(req.clone()),
            _ => None,
        })
        .collect()
}

fn recorded_time_range_request_bars(recorder: &Rc<RefCell<Vec<DataCommand>>>) -> Vec<RequestBars> {
    recorder
        .borrow()
        .iter()
        .filter_map(|cmd| match cmd {
            DataCommand::Request(RequestCommand::Bars(req)) => Some(req.clone()),
            _ => None,
        })
        .collect()
}

fn recorded_time_range_request_book_deltas(
    recorder: &Rc<RefCell<Vec<DataCommand>>>,
) -> Vec<RequestBookDeltas> {
    recorder
        .borrow()
        .iter()
        .filter_map(|cmd| match cmd {
            DataCommand::Request(RequestCommand::BookDeltas(req)) => Some(req.clone()),
            _ => None,
        })
        .collect()
}

fn recorded_time_range_request_book_depth(
    recorder: &Rc<RefCell<Vec<DataCommand>>>,
) -> Vec<RequestBookDepth> {
    recorder
        .borrow()
        .iter()
        .filter_map(|cmd| match cmd {
            DataCommand::Request(RequestCommand::BookDepth(req)) => Some(req.clone()),
            _ => None,
        })
        .collect()
}

fn recorded_time_range_request_funding_rates(
    recorder: &Rc<RefCell<Vec<DataCommand>>>,
) -> Vec<RequestFundingRates> {
    recorder
        .borrow()
        .iter()
        .filter_map(|cmd| match cmd {
            DataCommand::Request(RequestCommand::FundingRates(req)) => Some(req.clone()),
            _ => None,
        })
        .collect()
}

fn pipeline_bar(bar_type: BarType, ts: u64) -> Bar {
    Bar::new(
        bar_type,
        Price::from("1.0000"),
        Price::from("1.0001"),
        Price::from("0.9999"),
        Price::from("1.0000"),
        Quantity::from(1),
        UnixNanos::from(ts),
        UnixNanos::from(ts),
    )
}

fn pipeline_funding_rate(instrument_id: InstrumentId, ts: u64) -> FundingRateUpdate {
    FundingRateUpdate::new(
        instrument_id,
        "0.0001".parse().unwrap(),
        None,
        None,
        UnixNanos::from(ts),
        UnixNanos::from(ts),
    )
}

fn leg_bars_response(
    request_id: UUID4,
    bar_type: BarType,
    client_id: ClientId,
    bars: Vec<Bar>,
    start: Option<UnixNanos>,
    end: Option<UnixNanos>,
) -> DataResponse {
    DataResponse::Bars(BarsResponse::new(
        request_id,
        client_id,
        bar_type,
        bars,
        start,
        end,
        UnixNanos::default(),
        None,
    ))
}

fn leg_book_deltas_response(
    request_id: UUID4,
    instrument_id: InstrumentId,
    client_id: ClientId,
    deltas: Vec<OrderBookDelta>,
) -> DataResponse {
    DataResponse::BookDeltas(BookDeltasResponse::new(
        request_id,
        client_id,
        instrument_id,
        deltas,
        None,
        None,
        UnixNanos::default(),
        None,
    ))
}

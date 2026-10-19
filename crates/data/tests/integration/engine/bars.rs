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
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let config = DataEngineConfig {
        validate_data_sequence: true,
        ..DataEngineConfig::default()
    };

    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), Some(config));

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
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
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

    let mut data_engine = DataEngine::new(Rc::clone(&clock), Rc::clone(&cache), Some(config));

    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        Rc::clone(&cache),
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
            AggressorSide::Buy,
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
#[case::validation_disabled(false)]
#[case::validation_enabled(true)]
fn test_request_scoped_bar_aggregator_older_history_inserted_before_newer_live_bar(
    audusd_sim: CurrencyPair,
    stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
    #[case] validate_sequence: bool,
) {
    let _ = stub_msgbus;
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let instrument_id = audusd_sim.id;
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();

    let config = DataEngineConfig {
        validate_data_sequence: validate_sequence,
        ..DataEngineConfig::default()
    };

    let mut data_engine = DataEngine::new(clock, Rc::clone(&cache), Some(config));
    let test_clock: Rc<RefCell<VirtualClock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        test_clock,
        Rc::clone(&cache),
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
            AggressorSide::Buy,
            TradeId::new(trade_id),
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };

    // a live bar is cached before the requested history
    data_engine.process_data(Data::Trade(make_trade(2_000, "live-1")));
    assert_eq!(
        cache.borrow().bar(&bar_type).map(|bar| bar.ts_event),
        Some(UnixNanos::from(2_000)),
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
    data_engine.execute(DataCommand::Request(RequestCommand::Trades(request)));

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

    // the older requested aggregate is cached behind the newer live bar
    let stamps: Vec<_> = cache
        .borrow()
        .bars(&bar_type)
        .map(|bars| bars.iter().map(|bar| bar.ts_event).collect())
        .unwrap_or_default();
    assert_eq!(stamps, vec![UnixNanos::from(2_000), UnixNanos::from(1_000)]);

    // the live path continues to extend the series
    data_engine.process_data(Data::Trade(make_trade(3_000, "live-2")));

    let stamps: Vec<_> = cache
        .borrow()
        .bars(&bar_type)
        .map(|bars| bars.iter().map(|bar| bar.ts_event).collect())
        .unwrap_or_default();
    assert_eq!(
        stamps,
        vec![
            UnixNanos::from(3_000),
            UnixNanos::from(2_000),
            UnixNanos::from(1_000),
        ],
    );
}

#[rstest]
fn test_shared_internal_bars_retry_failed_source(
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
        MockSubscribeFailure::Trades,
        &mut data_engine,
    );
    data_engine.process(&InstrumentAny::CurrencyPair(audusd_sim.clone()) as &dyn Any);
    let bar_type = BarType::from("AUD/USD.SIM-1-MINUTE-LAST-INTERNAL");
    let first_command_id = UUID4::new();

    let subscribe = |command_id| {
        DataCommand::Subscribe(SubscribeCommand::Bars(SubscribeBars::new(
            bar_type,
            Some(client_id),
            Some(venue),
            command_id,
            UnixNanos::from(1),
            None,
            None,
        )))
    };

    data_engine.execute(subscribe(first_command_id));
    assert!(recorder.borrow().is_empty());

    data_engine.execute(subscribe(UUID4::new()));

    let recorded = recorder.borrow();
    assert_eq!(recorded.len(), 1);

    let DataCommand::Subscribe(SubscribeCommand::Trades(command)) = &recorded[0] else {
        panic!("expected a trade source subscription");
    };

    assert_eq!(command.instrument_id, audusd_sim.id);
    assert_eq!(command.client_id, Some(client_id));
    assert_eq!(command.venue, Some(venue));
    assert_eq!(command.ts_init, UnixNanos::from(1));
    assert_eq!(command.correlation_id, Some(first_command_id));
    #[cfg(feature = "streaming")]
    let expected_params = Some(client_subscription_params(Params::new()));
    #[cfg(not(feature = "streaming"))]
    let expected_params = None;
    assert_eq!(command.params, expected_params);
}

#[rstest]
#[case::two_levels(&[
    "AUD/USD.SIM-5-MINUTE-BID-INTERNAL@1-MINUTE-EXTERNAL",
    "AUD/USD.SIM-15-MINUTE-BID-INTERNAL@5-MINUTE-INTERNAL",
])]
#[case::three_levels(&[
    "AUD/USD.SIM-5-MINUTE-BID-INTERNAL@1-MINUTE-EXTERNAL",
    "AUD/USD.SIM-15-MINUTE-BID-INTERNAL@5-MINUTE-INTERNAL",
    "AUD/USD.SIM-1-HOUR-BID-INTERNAL@15-MINUTE-INTERNAL",
])]
fn test_unsubscribe_chained_composite_bars_releases_retained_source(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
    #[case] chain: &[&str],
) {
    // Releasing the top of the chain must walk down to the 5-minute source and free its
    // external 1-minute feed, not the quote feed another owner holds.
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

    let instrument_id = audusd_sim.id;
    let inst_any = InstrumentAny::CurrencyPair(audusd_sim);
    data_engine.process(&inst_any as &dyn Any);

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

    let bar_types: Vec<BarType> = chain
        .iter()
        .map(|bar_type| BarType::from(*bar_type))
        .collect();

    for bar_type in bar_types.iter().copied() {
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
    }

    recorder.borrow_mut().clear();

    // Each lower release defers while the next aggregator up the chain still consumes its topic
    for bar_type in bar_types.iter().copied() {
        data_engine.execute(DataCommand::Unsubscribe(UnsubscribeCommand::Bars(
            UnsubscribeBars::new(
                bar_type,
                Some(client_id),
                Some(venue),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ),
        )));
    }

    let recorded = recorder.borrow();
    assert_eq!(recorded.len(), 1);

    let DataCommand::Unsubscribe(UnsubscribeCommand::Bars(command)) = &recorded[0] else {
        panic!(
            "expected external source bars unsubscribe, was {:?}",
            recorded[0]
        );
    };

    assert_eq!(
        command.bar_type,
        BarType::from("AUD/USD.SIM-1-MINUTE-BID-EXTERNAL")
    );
    assert_eq!(command.client_id, Some(client_id));
    assert_eq!(data_engine.subscribed_quotes(), vec![instrument_id]);
    assert!(data_engine.subscribed_bars().is_empty());
}

#[rstest]
#[case::owned(false)]
#[case::borrowed(true)]
fn test_process_bar(
    #[case] borrowed: bool,
    data_engine: Rc<RefCell<DataEngine>>,
    data_client: DataClientAdapter,
) {
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
    dispatch_data(&mut data_engine, Data::Bar(bar), borrowed);
    let cache = &data_engine.cache().borrow();
    let messages = saver.get_messages();

    assert_eq!(cache.bar(&bar.bar_type), Some(bar).as_ref());
    assert_eq!(messages.len(), 1);
    assert!(messages.contains(&bar));
}

#[rstest]
fn test_trim_to_bounds_trims_bars(audusd_sim: CurrencyPair) {
    let instrument_id = audusd_sim.id;
    let bar_type = BarType::from(format!("{instrument_id}-1-MINUTE-LAST-INTERNAL").as_str());

    let make_bar = |ts: u64| {
        Bar::new(
            bar_type,
            Price::from("1.00000"),
            Price::from("1.00010"),
            Price::from("0.99990"),
            Price::from("1.00005"),
            Quantity::from("1"),
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )
    };

    let mut resp = DataResponse::Bars(BarsResponse::new(
        UUID4::new(),
        ClientId::test_default(),
        bar_type,
        vec![make_bar(1_000), make_bar(2_000), make_bar(3_000)],
        Some(UnixNanos::from(2_000)),
        Some(UnixNanos::from(3_000)),
        UnixNanos::default(),
        None,
    ));

    resp.trim_to_bounds();

    let DataResponse::Bars(bars) = resp else {
        panic!("expected Bars variant");
    };

    let ts_inits: Vec<u64> = bars.data.iter().map(|b| b.ts_init.as_u64()).collect();
    assert_eq!(ts_inits, vec![2_000, 3_000]);
}

#[rstest]
fn test_external_bars_release_after_final_owner(
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

    let bar_type = BarType::from("AUD/USD.SIM-1-MINUTE-LAST-EXTERNAL");
    let first_subscribe = DataCommand::Subscribe(SubscribeCommand::Bars(SubscribeBars::new(
        bar_type,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )));
    let second_subscribe = DataCommand::Subscribe(SubscribeCommand::Bars(SubscribeBars::new(
        bar_type,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )));
    data_engine.execute(first_subscribe);
    data_engine.execute(second_subscribe);
    assert_eq!(recorder.borrow().len(), 1);

    data_engine.execute(DataCommand::Unsubscribe(UnsubscribeCommand::Bars(
        UnsubscribeBars::new(
            bar_type,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ),
    )));
    assert_eq!(recorder.borrow().len(), 1);

    data_engine.execute(DataCommand::Unsubscribe(UnsubscribeCommand::Bars(
        UnsubscribeBars::new(
            bar_type,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ),
    )));

    let recorded = recorder.borrow();
    assert_eq!(recorded.len(), 2);
    assert!(matches!(
        &recorded[1],
        DataCommand::Unsubscribe(UnsubscribeCommand::Bars(_))
    ));
}

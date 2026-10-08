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
fn test_subscribe_spread_quotes_default_interval_publishes_on_timer(
    clock: Rc<RefCell<VirtualClock>>,
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
    let (exchange_handler, exchange_saver) =
        get_typed_message_saving_handler::<QuoteTick>(Some(Ustr::from("spread-exchange-timer")));
    msgbus::register_quote_endpoint(
        format!("SimulatedExchange.process_new_quote.{}", spread_id.venue).into(),
        exchange_handler,
    );

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
            .clock()
            .borrow()
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

    let exchange_quotes = exchange_saver.get_messages();
    assert_eq!(exchange_quotes.len(), 1);
    assert_eq!(exchange_quotes[0], spread_quotes[0]);
}

#[rstest]
fn test_subscribe_spread_quotes_with_zero_interval_publishes_spread_quote(
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

    let spread = generic_futures_spread();
    let spread_id = spread.id();
    let (leg_a, leg_b) = generic_futures_spread_legs();
    let spread_any = InstrumentAny::FuturesSpread(spread);
    data_engine.process(&spread_any as &dyn Any);

    let (handler, saver) =
        get_typed_message_saving_handler::<QuoteTick>(Some(Ustr::from("spread-quotes")));
    let spread_topic = switchboard::get_quotes_topic(spread_id);
    msgbus::subscribe_quotes(spread_topic.into(), handler, None);
    let (exchange_handler, exchange_saver) =
        get_typed_message_saving_handler::<QuoteTick>(Some(Ustr::from("spread-exchange")));
    msgbus::register_quote_endpoint(
        format!("SimulatedExchange.process_new_quote.{}", spread_id.venue).into(),
        exchange_handler,
    );

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

    let exchange_quotes = exchange_saver.get_messages();
    assert_eq!(exchange_quotes.len(), 1);
    assert_eq!(exchange_quotes[0], spread_quotes[0]);
}

#[rstest]
fn test_subscribe_spread_quotes_without_exchange_endpoint_publishes_spread_quote(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let data_engine = create_snapshot_test_engine(clock.clone(), cache.clone());
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

    let (handler, saver) = get_typed_message_saving_handler::<QuoteTick>(Some(Ustr::from(
        "spread-quotes-no-exchange",
    )));
    let spread_topic = switchboard::get_quotes_topic(spread_id);
    msgbus::subscribe_quotes(spread_topic.into(), handler, None);

    let exchange_endpoint = format!("SimulatedExchange.process_new_quote.{}", spread_id.venue);
    assert!(!msgbus::has_quote_endpoint(
        exchange_endpoint.as_str().into()
    ));

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

    let tap = Rc::new(RecordingSendTap::default());
    msgbus::set_bus_tap(tap.clone());

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

    msgbus::clear_bus_tap();

    let spread_quotes = saver.get_messages();
    assert_eq!(spread_quotes.len(), 1);
    assert_eq!(spread_quotes[0].instrument_id, spread_id);
    assert_eq!(spread_quotes[0].bid_price, Price::from("1.00"));
    assert_eq!(spread_quotes[0].ask_price, Price::from("3.00"));
    assert_eq!(spread_quotes[0].bid_size, Quantity::from(5));
    assert_eq!(spread_quotes[0].ask_size, Quantity::from(6));
    assert!(tap.send_endpoints().is_empty());
}

#[rstest]
fn test_unsubscribe_spread_quotes_stops_default_interval_timer(
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
            .clock()
            .borrow()
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
    assert!(data_engine.clock().borrow().timer_names().is_empty());
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
fn test_spread_quotes_release_after_final_owner_with_first_route(
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
    let spread = generic_futures_spread();
    let spread_id = spread.id();
    let (leg_a, leg_b) = generic_futures_spread_legs();
    data_engine.process(&InstrumentAny::FuturesSpread(spread) as &dyn Any);
    let mut first_params = spread_quote_params();
    first_params.insert("owner".to_string(), serde_json::json!(1));
    let mut second_params = spread_quote_params();
    second_params.insert("owner".to_string(), serde_json::json!(2));

    for params in [first_params.clone(), second_params.clone()] {
        data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Quotes(
            SubscribeQuotes::new(
                spread_id,
                Some(client_id),
                Some(venue),
                UUID4::new(),
                UnixNanos::default(),
                None,
                Some(params),
            ),
        )));
    }

    assert_eq!(recorder.borrow().len(), 2);

    let unsubscribe = |params| {
        DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(UnsubscribeQuotes::new(
            spread_id,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            Some(params),
        )))
    };

    data_engine.execute(unsubscribe(second_params.clone()));
    assert_eq!(recorder.borrow().len(), 2);
    assert_eq!(
        msgbus::exact_subscriber_count_quotes(switchboard::get_quotes_topic(leg_a)),
        1,
    );
    assert_eq!(
        msgbus::exact_subscriber_count_quotes(switchboard::get_quotes_topic(leg_b)),
        1,
    );

    data_engine.execute(unsubscribe(second_params));

    let recorded = recorder.borrow();

    let released = recorded
        .iter()
        .filter_map(|command| match command {
            DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(command)) => Some(command),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(released.len(), 2);
    assert_eq!(released[0].instrument_id, leg_a);
    assert_eq!(released[1].instrument_id, leg_b);
    assert_eq!(released[0].params.as_ref(), Some(&first_params));
    assert_eq!(released[1].params.as_ref(), Some(&first_params));
    assert_eq!(
        msgbus::exact_subscriber_count_quotes(switchboard::get_quotes_topic(leg_a)),
        0,
    );
    assert_eq!(
        msgbus::exact_subscriber_count_quotes(switchboard::get_quotes_topic(leg_b)),
        0,
    );
}

#[rstest]
fn test_shared_spread_quotes_retry_failed_leg(
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
        MockSubscribeFailure::Quotes,
        &mut data_engine,
    );
    let spread = generic_futures_spread();
    let spread_id = spread.id();
    let (leg_a, leg_b) = generic_futures_spread_legs();
    data_engine.process(&InstrumentAny::FuturesSpread(spread) as &dyn Any);
    let first_command_id = UUID4::new();

    let subscribe = |command_id| {
        DataCommand::Subscribe(SubscribeCommand::Quotes(SubscribeQuotes::new(
            spread_id,
            Some(client_id),
            Some(venue),
            command_id,
            UnixNanos::from(1),
            None,
            Some(spread_quote_params()),
        )))
    };

    data_engine.execute(subscribe(first_command_id));
    assert_eq!(recorder.borrow().len(), 1);

    data_engine.execute(subscribe(UUID4::new()));

    let recorded = recorder.borrow();

    let commands = recorded
        .iter()
        .map(|command| match command {
            DataCommand::Subscribe(SubscribeCommand::Quotes(command)) => command,
            other => panic!("expected a quote subscription, was {other:?}"),
        })
        .collect::<Vec<_>>();

    assert_eq!(commands.len(), 2);
    assert_eq!(commands[0].instrument_id, leg_b);
    assert_eq!(commands[1].instrument_id, leg_a);

    for command in commands {
        assert_eq!(command.client_id, Some(client_id));
        assert_eq!(command.venue, Some(venue));
        assert_eq!(command.ts_init, UnixNanos::from(1));
        assert_eq!(command.correlation_id, Some(first_command_id));
        assert_eq!(
            command.params,
            Some(client_subscription_params(spread_quote_params())),
        );
    }

    drop(recorded);

    let unsubscribe = || {
        DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(UnsubscribeQuotes::new(
            spread_id,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::from(2),
            None,
            Some(spread_quote_params()),
        )))
    };

    data_engine.execute(unsubscribe());
    assert_eq!(recorder.borrow().len(), 2);
    data_engine.execute(unsubscribe());

    let recorded = recorder.borrow();

    let commands = recorded
        .iter()
        .filter_map(|command| match command {
            DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(command)) => Some(command),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(commands.len(), 2);
    assert_eq!(commands[0].instrument_id, leg_a);
    assert_eq!(commands[1].instrument_id, leg_b);

    for command in commands {
        assert_eq!(command.client_id, Some(client_id));
        assert_eq!(command.venue, Some(venue));
        assert_eq!(command.ts_init, UnixNanos::from(2));
        assert_eq!(command.correlation_id, Some(first_command_id));
        assert_eq!(command.params, Some(spread_quote_params()));
    }
}

#[rstest]
fn test_spread_quotes_retain_existing_leg_sources(
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
    let spread = generic_futures_spread();
    let spread_id = spread.id();
    let (leg_a, leg_b) = generic_futures_spread_legs();
    data_engine.process(&InstrumentAny::FuturesSpread(spread) as &dyn Any);
    let params = spread_quote_params();
    let spread_command_id = UUID4::new();

    for leg_id in [leg_a, leg_b] {
        data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Quotes(
            SubscribeQuotes::new(
                leg_id,
                Some(client_id),
                Some(venue),
                UUID4::new(),
                UnixNanos::from(1),
                None,
                Some(params.clone()),
            ),
        )));
    }

    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::Quotes(
        SubscribeQuotes::new(
            spread_id,
            Some(client_id),
            Some(venue),
            spread_command_id,
            UnixNanos::from(2),
            None,
            Some(params.clone()),
        ),
    )));
    assert_eq!(recorder.borrow().len(), 2);

    for leg_id in [leg_a, leg_b] {
        data_engine.execute(DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(
            UnsubscribeQuotes::new(
                leg_id,
                Some(client_id),
                Some(venue),
                UUID4::new(),
                UnixNanos::from(3),
                None,
                Some(params.clone()),
            ),
        )));
    }

    assert_eq!(recorder.borrow().len(), 2);

    data_engine.execute(DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(
        UnsubscribeQuotes::new(
            spread_id,
            Some(client_id),
            Some(venue),
            UUID4::new(),
            UnixNanos::from(4),
            None,
            Some(params.clone()),
        ),
    )));

    let recorded = recorder.borrow();

    let commands = recorded
        .iter()
        .filter_map(|command| match command {
            DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(command)) => Some(command),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(commands.len(), 2);
    assert_eq!(commands[0].instrument_id, leg_a);
    assert_eq!(commands[1].instrument_id, leg_b);

    for command in commands {
        assert_eq!(command.client_id, Some(client_id));
        assert_eq!(command.venue, Some(venue));
        assert_eq!(command.ts_init, UnixNanos::from(4));
        assert_eq!(command.correlation_id, Some(spread_command_id));
        assert_eq!(command.params, Some(params.clone()));
    }
}

#[rstest]
fn test_reset_stops_spread_quote_timer_and_removes_leg_handlers(
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
            .clock()
            .borrow()
            .timer_names()
            .iter()
            .any(|name| *name == timer_name)
    );

    data_engine.reset();

    assert!(data_engine.clock().borrow().timer_names().is_empty());
    assert_eq!(
        msgbus::exact_subscriber_count_quotes(switchboard::get_quotes_topic(leg_a)),
        0
    );
    assert_eq!(
        msgbus::exact_subscriber_count_quotes(switchboard::get_quotes_topic(leg_b)),
        0
    );
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

#[derive(Default)]
struct RecordingSendTap {
    endpoints: Rc<RefCell<Vec<String>>>,
}

impl RecordingSendTap {
    fn send_endpoints(&self) -> Vec<String> {
        self.endpoints.borrow().clone()
    }
}

impl BusTap for RecordingSendTap {
    fn on_publish(&self, _topic: MStr<Topic>, _message: &dyn Any) {}

    fn on_send(&self, endpoint: MStr<Endpoint>, _message: &dyn Any) {
        self.endpoints.borrow_mut().push(endpoint.to_string());
    }
}

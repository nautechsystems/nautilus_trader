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
fn test_reset_clears_book_and_option_chain_state_and_allows_resubscribe(
    audusd_sim: CurrencyPair,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let _ = msgbus::get_message_bus();
    let data_engine = make_option_chain_engine(clock.clone(), cache.clone());

    let sim_recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock.clone(),
        cache.clone(),
        client_id,
        venue,
        None,
        &sim_recorder,
        &mut data_engine.borrow_mut(),
    );

    let deribit_client_id = ClientId::new("DERIBIT");
    let deribit_venue = Venue::new("DERIBIT");
    let deribit_recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache.clone(),
        deribit_client_id,
        deribit_venue,
        Some(deribit_venue),
        &deribit_recorder,
        &mut data_engine.borrow_mut(),
    );

    let call = make_btc_option("50000.000", OptionKind::Call);
    let put = make_btc_option("50000.000", OptionKind::Put);
    let call_id = call.id();
    let _ = cache.borrow_mut().add_instrument(call);
    let _ = cache.borrow_mut().add_instrument(put);

    let book_id = audusd_sim.id;
    let deltas_topic = switchboard::get_book_deltas_topic(book_id);
    let depth_topic = switchboard::get_book_depth_topic(book_id);
    let greeks_topic = switchboard::get_option_greeks_topic(call_id);
    let series_id = make_series_id();

    let subscribe_all = |engine: &Rc<RefCell<DataEngine>>| {
        engine
            .borrow_mut()
            .execute(DataCommand::Subscribe(SubscribeCommand::BookDeltas(
                SubscribeBookDeltas::new(
                    book_id,
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
        engine
            .borrow_mut()
            .execute(DataCommand::Subscribe(SubscribeCommand::BookSnapshots(
                SubscribeBookSnapshots::new(
                    book_id,
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
        engine.borrow_mut().execute(make_subscribe_option_chain(
            series_id,
            vec![Price::from("50000.000")],
            Some(deribit_client_id),
            Some(deribit_venue),
        ));
    };

    subscribe_all(&data_engine);

    assert_eq!(msgbus::subscriber_count_deltas(deltas_topic), 1);
    assert_eq!(msgbus::subscriber_count_depth(depth_topic), 0);
    assert!(!data_engine.borrow().subscribed_book_snapshots().is_empty());
    assert!(
        !data_engine
            .borrow()
            .clock()
            .borrow()
            .timer_names()
            .is_empty()
    );
    assert!(data_engine.borrow().has_option_chain_manager(&series_id));
    assert!(msgbus::exact_subscriber_count_option_greeks(greeks_topic) >= 1);

    data_engine.borrow_mut().reset();

    assert_eq!(msgbus::subscriber_count_deltas(deltas_topic), 0);
    assert_eq!(msgbus::subscriber_count_depth(depth_topic), 0);
    assert!(data_engine.borrow().subscribed_book_snapshots().is_empty());
    assert!(
        data_engine
            .borrow()
            .clock()
            .borrow()
            .timer_names()
            .is_empty()
    );
    assert!(!data_engine.borrow().has_option_chain_manager(&series_id));
    assert_eq!(data_engine.borrow().pending_option_chain_request_count(), 0);
    assert_eq!(
        msgbus::exact_subscriber_count_option_greeks(greeks_topic),
        0
    );

    subscribe_all(&data_engine);

    assert_eq!(msgbus::subscriber_count_deltas(deltas_topic), 1);
    assert_eq!(msgbus::subscriber_count_depth(depth_topic), 0);
    assert!(!data_engine.borrow().subscribed_book_snapshots().is_empty());
    assert!(
        !data_engine
            .borrow()
            .clock()
            .borrow()
            .timer_names()
            .is_empty()
    );
    assert!(data_engine.borrow().has_option_chain_manager(&series_id));
    assert!(msgbus::exact_subscriber_count_option_greeks(greeks_topic) >= 1);
}

#[rstest]
fn test_external_option_chain_releases_after_final_owner(
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
    let series_id = make_series_id();
    let mut first_params = Params::new();
    first_params.insert("owner".to_string(), serde_json::json!(1));
    let mut second_params = Params::new();
    second_params.insert("owner".to_string(), serde_json::json!(2));

    let subscribe = |command_id, strike, params| {
        SubscribeCommand::OptionChain(SubscribeOptionChain::new(
            series_id,
            StrikeRange::Fixed(vec![Price::from(strike)]),
            Some(1_000),
            command_id,
            UnixNanos::default(),
            Some(client_id),
            Some(venue),
            Some(params),
        ))
    };

    let (subscribe_handler, subscribe_saver) = get_any_saving_handler::<SubscribeCommand>(None);
    let (unsubscribe_handler, unsubscribe_saver) =
        get_any_saving_handler::<UnsubscribeCommand>(None);
    msgbus::subscribe_any(topic.as_str().into(), subscribe_handler, None);
    msgbus::subscribe_any(topic.as_str().into(), unsubscribe_handler, None);

    data_engine.execute(DataCommand::Subscribe(subscribe(
        UUID4::new(),
        "50000",
        first_params,
    )));
    data_engine.execute(DataCommand::Subscribe(subscribe(
        UUID4::new(),
        "51000",
        second_params.clone(),
    )));
    assert_eq!(subscribe_saver.get_messages().len(), 2);

    data_engine.execute(DataCommand::Unsubscribe(UnsubscribeCommand::OptionChain(
        UnsubscribeOptionChain::new(
            series_id,
            UUID4::new(),
            UnixNanos::from(1),
            Some(client_id),
            Some(venue),
        ),
    )));
    assert!(unsubscribe_saver.get_messages().is_empty());

    data_engine.execute(DataCommand::Unsubscribe(UnsubscribeCommand::OptionChain(
        UnsubscribeOptionChain::new(
            series_id,
            UUID4::new(),
            UnixNanos::from(2),
            Some(client_id),
            Some(venue),
        ),
    )));
    let commands = unsubscribe_saver.get_messages();

    let [UnsubscribeCommand::OptionChain(command)] = commands.as_slice() else {
        panic!("expected one final external option chain unsubscribe, was {commands:?}");
    };

    assert_eq!(command.series_id, series_id);
    assert_eq!(command.client_id, Some(client_id));
    assert_eq!(command.venue, Some(venue));
    assert_eq!(command.ts_init, UnixNanos::from(2));
    assert_eq!(command.params.as_ref(), Some(&second_params));
}

#[rstest]
fn test_external_option_chain_edit_moves_owner_to_new_client(
    _stub_msgbus: Rc<RefCell<MessageBus>>,
    client_id: ClientId,
    venue: Venue,
) {
    let edited_client_id = ClientId::from("EDITED-EXTERNAL-CLIENT");
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));

    let config = DataEngineConfig {
        external_clients: Some(vec![client_id, edited_client_id]),
        ..DataEngineConfig::default()
    };

    let mut data_engine = DataEngine::new(clock, cache, Some(config));
    let initial_topic = format!("commands.data.{client_id}");
    let edited_topic = format!("commands.data.{edited_client_id}");
    let series_id = make_series_id();
    let owner_id = UUID4::new();
    let mut initial_params = Params::new();
    initial_params.insert("route".to_string(), serde_json::json!(1));
    let mut edited_params = Params::new();
    edited_params.insert("route".to_string(), serde_json::json!(2));
    let (initial_unsubscribe_handler, initial_unsubscribe_saver) =
        get_any_saving_handler::<UnsubscribeCommand>(None);
    let (edited_subscribe_handler, edited_subscribe_saver) =
        get_any_saving_handler::<SubscribeCommand>(None);
    let (edited_unsubscribe_handler, edited_unsubscribe_saver) =
        get_any_saving_handler::<UnsubscribeCommand>(None);
    msgbus::subscribe_any(
        initial_topic.as_str().into(),
        initial_unsubscribe_handler,
        None,
    );
    msgbus::subscribe_any(edited_topic.as_str().into(), edited_subscribe_handler, None);
    msgbus::subscribe_any(
        edited_topic.as_str().into(),
        edited_unsubscribe_handler,
        None,
    );

    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::OptionChain(
        SubscribeOptionChain::new(
            series_id,
            StrikeRange::Fixed(vec![Price::from("50000")]),
            Some(1_000),
            owner_id,
            UnixNanos::from(1),
            Some(client_id),
            Some(venue),
            Some(initial_params.clone()),
        ),
    )));

    let mut edit = SubscribeOptionChain::new(
        series_id,
        StrikeRange::Fixed(vec![Price::from("51000")]),
        Some(2_000),
        UUID4::new(),
        UnixNanos::from(2),
        Some(edited_client_id),
        Some(venue),
        Some(edited_params.clone()),
    );
    edit.correlation_id = Some(owner_id);
    data_engine.execute(DataCommand::Subscribe(SubscribeCommand::OptionChain(edit)));

    let initial_commands = initial_unsubscribe_saver.get_messages();

    let [UnsubscribeCommand::OptionChain(initial_unsubscribe)] = initial_commands.as_slice() else {
        panic!("expected one unsubscribe from the old external client, was {initial_commands:?}");
    };

    assert_eq!(initial_unsubscribe.series_id, series_id);
    assert_eq!(initial_unsubscribe.client_id, Some(client_id));
    assert_eq!(initial_unsubscribe.ts_init, UnixNanos::from(2));
    assert_eq!(initial_unsubscribe.params.as_ref(), Some(&initial_params));
    let edited_commands = edited_subscribe_saver.get_messages();

    let [SubscribeCommand::OptionChain(edited_subscribe)] = edited_commands.as_slice() else {
        panic!("expected one subscribe to the new external client, was {edited_commands:?}");
    };

    assert_eq!(edited_subscribe.correlation_id, Some(owner_id));

    data_engine.execute(DataCommand::Unsubscribe(UnsubscribeCommand::OptionChain(
        UnsubscribeOptionChain::new(
            series_id,
            UUID4::new(),
            UnixNanos::from(3),
            Some(edited_client_id),
            Some(venue),
        ),
    )));
    let final_commands = edited_unsubscribe_saver.get_messages();

    let [UnsubscribeCommand::OptionChain(final_unsubscribe)] = final_commands.as_slice() else {
        panic!("expected one unsubscribe from the active external client, was {final_commands:?}");
    };

    assert_eq!(final_unsubscribe.series_id, series_id);
    assert_eq!(final_unsubscribe.client_id, Some(edited_client_id));
    assert_eq!(final_unsubscribe.ts_init, UnixNanos::from(3));
    assert_eq!(final_unsubscribe.params.as_ref(), Some(&edited_params));
}

#[rstest]
fn test_subscribe_option_chain_fixed_range_creates_manager(
    clock: Rc<RefCell<VirtualClock>>,
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
fn test_subscribe_option_chain_rejects_zero_snapshot_interval(
    clock: Rc<RefCell<VirtualClock>>,
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
    let _ = cache
        .borrow_mut()
        .add_instrument(make_btc_option("50000.000", OptionKind::Call));

    let series_id = make_series_id();
    data_engine
        .borrow_mut()
        .execute(DataCommand::Subscribe(SubscribeCommand::OptionChain(
            SubscribeOptionChain::new(
                series_id,
                StrikeRange::Fixed(vec![Price::from("50000.000")]),
                Some(0),
                UUID4::new(),
                UnixNanos::default(),
                Some(client_id),
                Some(venue),
                None,
            ),
        )));

    assert!(!data_engine.borrow().has_option_chain_manager(&series_id));
    assert!(recorder.borrow().is_empty());
}

#[rstest]
fn test_subscribe_option_chain_filters_by_underlying(
    clock: Rc<RefCell<VirtualClock>>,
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
fn test_option_chain_new_instrument_uses_subscription_client(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let _ = msgbus::get_message_bus();
    let data_engine = make_option_chain_engine(clock.clone(), cache.clone());
    let explicit_client_id = ClientId::new("DERIBIT-EXPLICIT");
    let routed_client_id = ClientId::new("DERIBIT-ROUTED");
    let venue = Venue::new("DERIBIT");
    let explicit_recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));
    let routed_recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));
    register_mock_client(
        clock.clone(),
        cache.clone(),
        explicit_client_id,
        venue,
        None,
        &explicit_recorder,
        &mut data_engine.borrow_mut(),
    );
    register_mock_client(
        clock,
        cache.clone(),
        routed_client_id,
        venue,
        Some(venue),
        &routed_recorder,
        &mut data_engine.borrow_mut(),
    );
    let _ = cache
        .borrow_mut()
        .add_instrument(make_btc_option("50000.000", OptionKind::Call));

    let series_id = make_series_id();
    data_engine
        .borrow_mut()
        .execute(make_subscribe_option_chain(
            series_id,
            vec![Price::from("50000.000")],
            Some(explicit_client_id),
            Some(venue),
        ));
    explicit_recorder.borrow_mut().clear();
    routed_recorder.borrow_mut().clear();

    let put = make_btc_option("50000.000", OptionKind::Put);
    let put_id = put.id();
    data_engine.borrow_mut().process(&put);

    let recorded = explicit_recorder.borrow();
    assert_eq!(recorded.len(), 3);
    assert!(recorded.iter().any(|cmd| matches!(
        cmd,
        DataCommand::Subscribe(SubscribeCommand::Quotes(cmd)) if cmd.instrument_id == put_id
    )));
    assert!(recorded.iter().any(|cmd| matches!(
        cmd,
        DataCommand::Subscribe(SubscribeCommand::OptionGreeks(cmd))
            if cmd.instrument_id == put_id
    )));
    assert!(recorded.iter().any(|cmd| matches!(
        cmd,
        DataCommand::Subscribe(SubscribeCommand::InstrumentStatus(cmd))
            if cmd.instrument_id == put_id
    )));
    drop(recorded);
    assert!(routed_recorder.borrow().is_empty());
    explicit_recorder.borrow_mut().clear();

    data_engine
        .borrow_mut()
        .process_data(Data::InstrumentStatus(InstrumentStatus::new(
            put_id,
            MarketStatusAction::Close,
            UnixNanos::from(1),
            UnixNanos::from(2),
            None,
            None,
            Some(false),
            Some(false),
            None,
        )));

    let recorded = explicit_recorder.borrow();
    assert_eq!(recorded.len(), 3);
    assert!(recorded.iter().any(|cmd| matches!(
        cmd,
        DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(cmd))
            if cmd.instrument_id == put_id
    )));
    assert!(recorded.iter().any(|cmd| matches!(
        cmd,
        DataCommand::Unsubscribe(UnsubscribeCommand::OptionGreeks(cmd))
            if cmd.instrument_id == put_id
    )));
    assert!(recorded.iter().any(|cmd| matches!(
        cmd,
        DataCommand::Unsubscribe(UnsubscribeCommand::InstrumentStatus(cmd))
            if cmd.instrument_id == put_id
    )));
    assert!(routed_recorder.borrow().is_empty());
}

#[rstest]
fn test_option_chain_out_of_range_listing_is_not_subscribed(
    clock: Rc<RefCell<VirtualClock>>,
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
    let _ = cache
        .borrow_mut()
        .add_instrument(make_btc_option("50000.000", OptionKind::Call));

    let series_id = make_series_id();
    data_engine
        .borrow_mut()
        .execute(make_subscribe_option_chain(
            series_id,
            vec![Price::from("50000.000")],
            Some(client_id),
            Some(venue),
        ));
    recorder.borrow_mut().clear();

    // A new listing outside the fixed strikes joins the series without venue feeds
    data_engine
        .borrow_mut()
        .process(&make_btc_option("60000.000", OptionKind::Call));
    assert!(recorder.borrow().is_empty());

    data_engine
        .borrow_mut()
        .execute(make_unsubscribe_option_chain(
            series_id,
            Some(client_id),
            Some(venue),
        ));

    let data_engine = data_engine.borrow();
    assert!(data_engine.subscribed_quotes().is_empty());
    assert!(data_engine.subscribed_instrument_status().is_empty());
    assert!(
        data_engine.get_clients()[0]
            .subscriptions_option_greeks
            .is_empty()
    );
}

#[rstest]
fn test_unsubscribe_option_chain_tears_down(
    clock: Rc<RefCell<VirtualClock>>,
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
fn test_option_chain_manager_survives_partial_retirement(
    clock: Rc<RefCell<VirtualClock>>,
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
    let _ = cache
        .borrow_mut()
        .add_instrument(make_btc_option("50000.000", OptionKind::Call));
    let series_id = make_series_id();
    let topic = switchboard::get_option_chain_topic(series_id);
    let (first_handler, _first_saver) = get_typed_message_saving_handler::<OptionChainSlice>(Some(
        Ustr::from("first-option-chain-owner"),
    ));
    let (second_handler, _second_saver) = get_typed_message_saving_handler::<OptionChainSlice>(
        Some(Ustr::from("second-option-chain-owner")),
    );
    msgbus::subscribe_option_chain(topic.into(), first_handler.clone(), None);
    data_engine
        .borrow_mut()
        .execute(make_subscribe_option_chain(
            series_id,
            vec![Price::from("50000.000")],
            Some(client_id),
            Some(venue),
        ));
    msgbus::subscribe_option_chain(topic.into(), second_handler.clone(), None);
    data_engine
        .borrow_mut()
        .execute(make_subscribe_option_chain(
            series_id,
            vec![Price::from("50000.000")],
            Some(client_id),
            Some(venue),
        ));
    recorder.borrow_mut().clear();

    msgbus::unsubscribe_option_chain(topic.into(), &first_handler);
    data_engine
        .borrow_mut()
        .execute(make_unsubscribe_option_chain(
            series_id,
            Some(client_id),
            Some(venue),
        ));

    assert!(data_engine.borrow().has_option_chain_manager(&series_id));
    assert!(recorder.borrow().is_empty());

    msgbus::unsubscribe_option_chain(topic.into(), &second_handler);
    data_engine
        .borrow_mut()
        .execute(make_unsubscribe_option_chain(
            series_id,
            Some(client_id),
            Some(venue),
        ));

    assert!(!data_engine.borrow().has_option_chain_manager(&series_id));
    assert!(recorder.borrow().iter().any(|command| matches!(
        command,
        DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(_))
    )));
}

#[rstest]
#[case::retire(false)]
#[case::edit(true)]
fn test_option_chain_settles_rebalance_before_retirement(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    #[case] edit: bool,
) {
    let _ = msgbus::get_message_bus();
    let engine = make_option_chain_engine(clock.clone(), cache.clone());
    let client_id = ClientId::new("DERIBIT");
    let venue = Venue::new("DERIBIT");
    let recorder = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock.clone(),
        cache.clone(),
        client_id,
        venue,
        Some(venue),
        &recorder,
        &mut engine.borrow_mut(),
    );
    let old = make_btc_option("50000.000", OptionKind::Call);
    let next = make_btc_option("51000.000", OptionKind::Call);
    let old_id = old.id();
    let next_id = next.id();
    cache.borrow_mut().add_instrument(old).unwrap();
    cache.borrow_mut().add_instrument(next).unwrap();
    engine
        .borrow_mut()
        .execute(DataCommand::Subscribe(SubscribeCommand::Quotes(
            SubscribeQuotes::new(
                next_id,
                Some(client_id),
                Some(venue),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ),
        )));
    let series_id = make_series_id();
    engine
        .borrow_mut()
        .execute(DataCommand::Subscribe(SubscribeCommand::OptionChain(
            SubscribeOptionChain::new(
                series_id,
                StrikeRange::AtmRelative {
                    strikes_above: 0,
                    strikes_below: 0,
                },
                Some(1000),
                UUID4::new(),
                UnixNanos::default(),
                Some(client_id),
                Some(venue),
                None,
            ),
        )));

    let request_id = option_chain_reference_price_request_id(&recorder);
    engine
        .borrow_mut()
        .response(DataResponse::OptionChainReferencePrice(
            OptionChainReferencePriceResponse::new(
                request_id,
                client_id,
                series_id,
                Some(Price::from("50000.000")),
                UnixNanos::from(1),
                None,
            ),
        ));
    engine
        .borrow_mut()
        .process_data(Data::OptionGreeks(make_option_chain_greeks(
            old_id, 51000.0,
        )));
    recorder.borrow_mut().clear();

    let events = clock
        .borrow_mut()
        .advance_time(UnixNanos::from(6_000_000_000_u64), true);
    let handlers = clock.borrow().match_handlers(events);
    assert!(!handlers.is_empty());

    for handler in handlers {
        handler.callback.call(handler.event);
    }

    assert!(recorder.borrow().is_empty());

    if edit {
        engine.borrow_mut().execute(make_subscribe_option_chain(
            series_id,
            vec![Price::from("50000.000")],
            Some(client_id),
            Some(venue),
        ));
    }

    engine.borrow_mut().execute(make_unsubscribe_option_chain(
        series_id,
        Some(client_id),
        Some(venue),
    ));
    let commands_at_retirement = recorder.borrow().clone();
    engine
        .borrow_mut()
        .process_data(Data::OptionGreeks(make_option_chain_greeks(
            old_id, 51000.0,
        )));

    assert_eq!(
        recorder.borrow().as_slice(),
        commands_at_retirement.as_slice()
    );
    assert!(!commands_at_retirement.iter().any(|command| matches!(command,
        DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(command)) if command.instrument_id == next_id
    )));
    assert!(!engine.borrow().has_option_chain_manager(&series_id));
    assert_eq!(engine.borrow().subscribed_quotes(), vec![next_id]);
    assert!(
        engine
            .borrow_mut()
            .get_client(Some(&client_id), Some(&venue))
            .unwrap()
            .subscriptions_option_greeks
            .is_empty()
    );
    assert!(engine.borrow().subscribed_instrument_status().is_empty());
    assert_eq!(clock.borrow().timer_count(), 0);
}

#[rstest]
fn test_pending_option_chain_survives_partial_retirement(
    clock: Rc<RefCell<VirtualClock>>,
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
    let _ = cache
        .borrow_mut()
        .add_instrument(make_btc_option("50000.000", OptionKind::Call));
    let series_id = make_series_id();
    let topic = switchboard::get_option_chain_topic(series_id);
    let (first_handler, _first_saver) = get_typed_message_saving_handler::<OptionChainSlice>(Some(
        Ustr::from("first-pending-option-chain-owner"),
    ));
    let (second_handler, _second_saver) = get_typed_message_saving_handler::<OptionChainSlice>(
        Some(Ustr::from("second-pending-option-chain-owner")),
    );

    let subscribe = || {
        DataCommand::Subscribe(SubscribeCommand::OptionChain(SubscribeOptionChain::new(
            series_id,
            StrikeRange::AtmRelative {
                strikes_above: 2,
                strikes_below: 2,
            },
            Some(1_000),
            UUID4::new(),
            UnixNanos::default(),
            Some(client_id),
            Some(venue),
            None,
        )))
    };

    msgbus::subscribe_option_chain(topic.into(), first_handler.clone(), None);
    data_engine.borrow_mut().execute(subscribe());
    msgbus::subscribe_option_chain(topic.into(), second_handler.clone(), None);
    data_engine.borrow_mut().execute(subscribe());
    assert_eq!(data_engine.borrow().pending_option_chain_request_count(), 1);

    msgbus::unsubscribe_option_chain(topic.into(), &first_handler);
    data_engine
        .borrow_mut()
        .execute(make_unsubscribe_option_chain(
            series_id,
            Some(client_id),
            Some(venue),
        ));

    assert_eq!(data_engine.borrow().pending_option_chain_request_count(), 1);

    msgbus::unsubscribe_option_chain(topic.into(), &second_handler);
    data_engine
        .borrow_mut()
        .execute(make_unsubscribe_option_chain(
            series_id,
            Some(client_id),
            Some(venue),
        ));

    assert_eq!(data_engine.borrow().pending_option_chain_request_count(), 0);
}

#[rstest]
fn test_option_chain_client_edit_releases_old_and_active_routes(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let _ = msgbus::get_message_bus();
    let data_engine = make_option_chain_engine(clock.clone(), cache.clone());
    let venue = Venue::new("DERIBIT");
    let first_client_id = ClientId::new("DERIBIT-FIRST");
    let second_client_id = ClientId::new("DERIBIT-SECOND");
    let first_recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));
    let second_recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));
    register_mock_client(
        clock.clone(),
        cache.clone(),
        first_client_id,
        venue,
        None,
        &first_recorder,
        &mut data_engine.borrow_mut(),
    );
    register_mock_client(
        clock,
        cache.clone(),
        second_client_id,
        venue,
        None,
        &second_recorder,
        &mut data_engine.borrow_mut(),
    );
    let _ = cache
        .borrow_mut()
        .add_instrument(make_btc_option("50000.000", OptionKind::Call));
    let series_id = make_series_id();
    let strikes = vec![Price::from("50000.000")];

    data_engine
        .borrow_mut()
        .execute(make_subscribe_option_chain(
            series_id,
            strikes.clone(),
            Some(first_client_id),
            Some(venue),
        ));
    data_engine
        .borrow_mut()
        .execute(make_subscribe_option_chain(
            series_id,
            strikes,
            Some(second_client_id),
            Some(venue),
        ));

    assert!(first_recorder.borrow().iter().any(|command| matches!(
        command,
        DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(command))
            if command.client_id == Some(first_client_id)
    )));
    assert!(
        !second_recorder
            .borrow()
            .iter()
            .any(|command| matches!(command, DataCommand::Unsubscribe(_)))
    );

    data_engine
        .borrow_mut()
        .execute(make_unsubscribe_option_chain(
            series_id,
            Some(first_client_id),
            Some(venue),
        ));

    assert!(second_recorder.borrow().iter().any(|command| matches!(
        command,
        DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(command))
            if command.client_id == Some(second_client_id)
    )));
}

#[rstest]
fn test_unsubscribe_option_chain_not_subscribed_does_not_panic(
    clock: Rc<RefCell<VirtualClock>>,
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
    clock: Rc<RefCell<VirtualClock>>,
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
    clock: Rc<RefCell<VirtualClock>>,
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
        data_engine
            .borrow()
            .cache()
            .borrow()
            .instrument_status(&call_id),
        Some(&status),
    );
    assert_eq!(quote_unsubs, expected_quote_unsubs);
    assert_eq!(greeks_unsubs, expected_greeks_unsubs);
}

#[rstest]
#[case::quote("quote")]
#[case::greeks("greeks")]
fn test_option_chain_market_data_at_expiry_expires_instrument(
    #[case] data_kind: &str,
    clock: Rc<RefCell<VirtualClock>>,
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

    recorder.borrow_mut().clear();

    match data_kind {
        "quote" => {
            let quote = QuoteTick::new(
                call_id,
                Price::from("100.00"),
                Price::from("101.00"),
                Quantity::from("1.0"),
                Quantity::from("1.0"),
                series_id.expiration_ns,
                series_id.expiration_ns,
            );
            data_engine.borrow_mut().process_data(Data::Quote(quote));
        }
        "greeks" => {
            let greeks = OptionGreeks {
                instrument_id: call_id,
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
                ts_event: series_id.expiration_ns,
                ts_init: series_id.expiration_ns,
            };

            data_engine.borrow_mut().process(&greeks);
        }
        other => panic!("unknown data kind: {other}"),
    }

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

    let status_unsubs = recorded
        .iter()
        .filter(|cmd| {
            matches!(
                cmd,
                DataCommand::Unsubscribe(UnsubscribeCommand::InstrumentStatus(_))
            )
        })
        .count();

    assert!(data_engine.borrow().has_option_chain_manager(&series_id));
    assert_eq!(quote_unsubs, 1);
    assert_eq!(greeks_unsubs, 1);
    assert_eq!(status_unsubs, 1);
}

#[rstest]
fn test_subscribe_option_chain_atm_relative_requests_reference_price(
    clock: Rc<RefCell<VirtualClock>>,
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

    // Add multiple instruments so sample selection must be stable.
    let call = make_btc_option("50000.000", OptionKind::Call);
    let put = make_btc_option("50000.000", OptionKind::Put);
    let call_id = call.id();
    let put_id = put.id();
    let future = FuturesContract::builder()
        .instrument_id(InstrumentId::from("AAA.DERIBIT"))
        .raw_symbol(Symbol::from("AAA"))
        .asset_class(AssetClass::Cryptocurrency)
        .exchange(Ustr::from("DERIBIT"))
        .underlying(Ustr::from("BTC"))
        .activation_ns(UnixNanos::default())
        .expiration_ns(make_series_id().expiration_ns)
        .currency(Currency::from("BTC"))
        .price_precision(2)
        .price_increment(Price::from("0.01"))
        .multiplier(Quantity::from(1))
        .lot_size(Quantity::from(1))
        .ts_event(UnixNanos::default())
        .ts_init(UnixNanos::default())
        .build()
        .unwrap();
    let _ = cache.borrow_mut().add_instrument(call);
    let _ = cache.borrow_mut().add_instrument(put);
    let _ = cache
        .borrow_mut()
        .add_instrument(InstrumentAny::FuturesContract(future));

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

    // ATM-relative should trigger a reference price request instead of immediate subscriptions
    let recorded = recorder.borrow();

    let reference_price_requests: Vec<&RequestOptionChainReferencePrice> = recorded
        .iter()
        .filter_map(|cmd| match cmd {
            DataCommand::Request(RequestCommand::OptionChainReferencePrice(request)) => {
                Some(request)
            }
            _ => None,
        })
        .collect();

    assert_eq!(reference_price_requests.len(), 1);
    let request = reference_price_requests[0];
    assert_eq!(request.series_id, series_id);
    assert_eq!(request.instrument_id, std::cmp::min(call_id, put_id));
    assert_eq!(request.client_id, Some(client_id));
    assert_eq!(request.params, None);

    // No direct quote subscriptions yet, deferred until the reference price response
    let quote_subs = recorded
        .iter()
        .filter(|cmd| matches!(cmd, DataCommand::Subscribe(SubscribeCommand::Quotes(_))))
        .count();
    assert_eq!(
        quote_subs, 0,
        "No quote subscriptions before reference price bootstrap"
    );
}

#[rstest]
#[case::atm_relative(StrikeRange::AtmRelative {
    strikes_above: 1,
    strikes_below: 1,
})]
#[case::delta(StrikeRange::Delta {
    target: 0.25,
    tolerance: 0.05,
})]
fn test_option_chain_reference_price_response_bootstraps_dynamic_range(
    #[case] strike_range: StrikeRange,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let _ = msgbus::get_message_bus();
    let data_engine = make_option_chain_engine(clock.clone(), cache.clone());
    let client_id = ClientId::new("DERIBIT");
    let venue = Venue::new("DERIBIT");
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));
    register_mock_client(
        clock.clone(),
        cache.clone(),
        client_id,
        venue,
        Some(venue),
        &recorder,
        &mut data_engine.borrow_mut(),
    );

    for strike in ["45000.000", "50000.000", "55000.000"] {
        let _ = cache
            .borrow_mut()
            .add_instrument(make_btc_option(strike, OptionKind::Call));
        let _ = cache
            .borrow_mut()
            .add_instrument(make_btc_option(strike, OptionKind::Put));
    }

    let series_id = make_series_id();
    data_engine
        .borrow_mut()
        .execute(DataCommand::Subscribe(SubscribeCommand::OptionChain(
            SubscribeOptionChain::new(
                series_id,
                strike_range,
                None,
                UUID4::new(),
                UnixNanos::default(),
                Some(client_id),
                Some(venue),
                None,
            ),
        )));
    let request_id = option_chain_reference_price_request_id(&recorder);
    recorder.borrow_mut().clear();

    data_engine
        .borrow_mut()
        .response(DataResponse::OptionChainReferencePrice(
            OptionChainReferencePriceResponse::new(
                request_id,
                client_id,
                series_id,
                Some(Price::from("50000.000")),
                UnixNanos::from(1),
                None,
            ),
        ));

    let quote_subscriptions = recorder
        .borrow()
        .iter()
        .filter(|cmd| matches!(cmd, DataCommand::Subscribe(SubscribeCommand::Quotes(_))))
        .count();
    assert_eq!(data_engine.borrow().pending_option_chain_request_count(), 0);
    assert!(data_engine.borrow().has_option_chain_manager(&series_id));
    assert_eq!(clock.borrow().timer_count(), 0);
    assert_eq!(quote_subscriptions, 6);
}

#[rstest]
fn test_option_chain_without_sample_bootstraps_from_live_data(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let _ = msgbus::get_message_bus();
    let data_engine = make_option_chain_engine(clock.clone(), cache.clone());
    let client_id = ClientId::new("DERIBIT");
    let venue = Venue::new("DERIBIT");
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));
    register_mock_client(
        clock.clone(),
        cache,
        client_id,
        venue,
        Some(venue),
        &recorder,
        &mut data_engine.borrow_mut(),
    );

    let series_id = make_series_id();
    data_engine
        .borrow_mut()
        .execute(make_subscribe_option_chain_atm(series_id, client_id, venue));

    assert!(recorder.borrow().is_empty());
    assert_eq!(data_engine.borrow().pending_option_chain_request_count(), 0);
    assert!(data_engine.borrow().has_option_chain_manager(&series_id));
    assert_eq!(clock.borrow().timer_count(), 0);
}

#[rstest]
fn test_unsubscribe_option_chain_cancels_pending_reference_price_request(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let _ = msgbus::get_message_bus();
    let data_engine = make_option_chain_engine(clock.clone(), cache.clone());
    let client_id = ClientId::new("DERIBIT");
    let venue = Venue::new("DERIBIT");
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));
    register_mock_client(
        clock.clone(),
        cache.clone(),
        client_id,
        venue,
        Some(venue),
        &recorder,
        &mut data_engine.borrow_mut(),
    );
    let _ = cache
        .borrow_mut()
        .add_instrument(make_btc_option("50000.000", OptionKind::Call));

    let series_id = make_series_id();
    data_engine
        .borrow_mut()
        .execute(make_subscribe_option_chain_atm(series_id, client_id, venue));
    let request_id = option_chain_reference_price_request_id(&recorder);

    let other_series_id = OptionSeriesId::new(
        venue,
        Ustr::from("BTC"),
        Ustr::from("BTC"),
        UnixNanos::from(series_id.expiration_ns.as_u64() + 1),
    );
    data_engine
        .borrow_mut()
        .response(DataResponse::OptionChainReferencePrice(
            OptionChainReferencePriceResponse::new(
                request_id,
                client_id,
                other_series_id,
                Some(Price::from("50000.000")),
                UnixNanos::from(1),
                None,
            ),
        ));
    assert_eq!(data_engine.borrow().pending_option_chain_request_count(), 1);
    assert!(!data_engine.borrow().has_option_chain_manager(&series_id));

    data_engine
        .borrow_mut()
        .execute(make_unsubscribe_option_chain(
            series_id,
            Some(client_id),
            Some(venue),
        ));
    data_engine
        .borrow_mut()
        .response(DataResponse::OptionChainReferencePrice(
            OptionChainReferencePriceResponse::new(
                request_id,
                client_id,
                series_id,
                Some(Price::from("50000.000")),
                UnixNanos::from(1),
                None,
            ),
        ));

    assert_eq!(data_engine.borrow().pending_option_chain_request_count(), 0);
    assert!(!data_engine.borrow().has_option_chain_manager(&series_id));
    assert_eq!(clock.borrow().timer_count(), 0);
}

#[rstest]
fn test_option_chain_reference_price_timeout_bootstraps_from_live_data(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let _ = msgbus::get_message_bus();
    let data_engine = make_option_chain_engine(clock.clone(), cache.clone());
    let client_id = ClientId::new("DERIBIT");
    let venue = Venue::new("DERIBIT");
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));
    register_mock_client(
        clock.clone(),
        cache.clone(),
        client_id,
        venue,
        Some(venue),
        &recorder,
        &mut data_engine.borrow_mut(),
    );
    let _ = cache
        .borrow_mut()
        .add_instrument(make_btc_option("50000.000", OptionKind::Call));

    let series_id = make_series_id();
    data_engine
        .borrow_mut()
        .execute(make_subscribe_option_chain_atm(series_id, client_id, venue));
    let timer_name = clock
        .borrow()
        .timer_names()
        .first()
        .map(|name| (*name).to_owned())
        .expect("reference price timeout should be scheduled");
    let timeout_ns = clock
        .borrow()
        .next_time_ns(&timer_name)
        .expect("reference price timeout should be scheduled");
    assert_eq!(data_engine.borrow().pending_option_chain_request_count(), 1);

    let advance_ns = timeout_ns.as_u64() - clock.borrow().timestamp_ns().as_u64();
    advance_clock_and_dispatch(&clock, advance_ns);

    assert_eq!(data_engine.borrow().pending_option_chain_request_count(), 0);
    assert!(data_engine.borrow().has_option_chain_manager(&series_id));
    assert_eq!(
        recorder
            .borrow()
            .iter()
            .filter(|cmd| matches!(
                cmd,
                DataCommand::Subscribe(SubscribeCommand::OptionGreeks(_))
            ))
            .count(),
        1
    );
}

#[rstest]
fn test_option_chain_reference_price_request_error_subscribes_bootstrap_greeks(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let _ = msgbus::get_message_bus();
    let data_engine = make_option_chain_engine(clock, cache.clone());
    let client_id = ClientId::new("DERIBIT");
    let venue = Venue::new("DERIBIT");
    let failing_client =
        FailingRequestDataClient::new(client_id, Some(venue), "request dispatch failed");
    let adapter =
        DataClientAdapter::new(client_id, Some(venue), true, true, Box::new(failing_client));
    data_engine
        .borrow_mut()
        .register_client(adapter, Some(venue));
    let sample_id = InstrumentId::from("BTC-20240101-50000.000-C.DERIBIT");
    let _ = cache
        .borrow_mut()
        .add_instrument(make_btc_option("50000.000", OptionKind::Call));

    let series_id = make_series_id();
    data_engine
        .borrow_mut()
        .execute(make_subscribe_option_chain_atm(series_id, client_id, venue));

    assert_eq!(data_engine.borrow().pending_option_chain_request_count(), 0);
    assert!(data_engine.borrow().has_option_chain_manager(&series_id));
    assert_eq!(
        msgbus::exact_subscriber_count_option_greeks(switchboard::get_option_greeks_topic(
            sample_id
        )),
        1
    );
    let sample_is_subscribed = data_engine
        .borrow_mut()
        .get_client(Some(&client_id), Some(&venue))
        .is_some_and(|client| client.subscriptions_option_greeks.contains(&sample_id));
    assert!(sample_is_subscribed);
}

#[rstest]
fn test_option_chain_reference_price_timeout_tracks_concurrent_requests(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let _ = msgbus::get_message_bus();
    let data_engine = make_option_chain_engine(clock.clone(), cache.clone());
    let client_id = ClientId::new("DERIBIT");
    let venue = Venue::new("DERIBIT");
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));
    register_mock_client(
        clock.clone(),
        cache.clone(),
        client_id,
        venue,
        Some(venue),
        &recorder,
        &mut data_engine.borrow_mut(),
    );

    let first_series = make_series_id();
    let second_expiration = first_series.expiration_ns + DurationNanos::from_secs(5);

    let second_series = OptionSeriesId::new(
        venue,
        Ustr::from("BTC"),
        Ustr::from("BTC"),
        second_expiration,
    );
    let _ = cache
        .borrow_mut()
        .add_instrument(make_btc_option("50000.000", OptionKind::Call));
    let _ = cache.borrow_mut().add_instrument(make_crypto_option(
        "BTC-20240102-50000-C.DERIBIT",
        "BTC",
        "BTC",
        "50000.000",
        OptionKind::Call,
        second_expiration,
    ));

    data_engine
        .borrow_mut()
        .execute(make_subscribe_option_chain_atm(
            first_series,
            client_id,
            venue,
        ));
    let timer_name = clock
        .borrow()
        .timer_names()
        .first()
        .map(|name| (*name).to_owned())
        .expect("first timeout should be scheduled");
    let first_deadline = clock
        .borrow()
        .next_time_ns(&timer_name)
        .expect("first timeout should be scheduled");

    advance_clock_and_dispatch(&clock, 10 * NANOSECONDS_IN_SECOND);
    data_engine
        .borrow_mut()
        .execute(make_subscribe_option_chain_atm(
            second_series,
            client_id,
            venue,
        ));

    assert_eq!(clock.borrow().timer_count(), 1);
    assert_eq!(
        clock.borrow().next_time_ns(&timer_name),
        Some(first_deadline)
    );

    let advance_ns = first_deadline.as_u64() - clock.borrow().timestamp_ns().as_u64();
    advance_clock_and_dispatch(&clock, advance_ns);

    assert!(data_engine.borrow().has_option_chain_manager(&first_series));
    assert!(
        !data_engine
            .borrow()
            .has_option_chain_manager(&second_series)
    );
    assert_eq!(data_engine.borrow().pending_option_chain_request_count(), 1);
    assert_eq!(clock.borrow().timer_count(), 1);

    let second_deadline = clock
        .borrow()
        .next_time_ns(&timer_name)
        .expect("second timeout should be scheduled");
    let advance_ns = second_deadline.as_u64() - clock.borrow().timestamp_ns().as_u64();
    advance_clock_and_dispatch(&clock, advance_ns);

    assert!(
        data_engine
            .borrow()
            .has_option_chain_manager(&second_series)
    );
    assert_eq!(data_engine.borrow().pending_option_chain_request_count(), 0);
    assert_eq!(clock.borrow().timer_count(), 0);
}

#[rstest]
fn test_option_chain_greeks_bootstrap_releases_inactive_sample(
    clock: Rc<RefCell<VirtualClock>>,
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

    for strike in ["45000.000", "50000.000", "55000.000"] {
        let _ = cache
            .borrow_mut()
            .add_instrument(make_btc_option(strike, OptionKind::Call));
        let _ = cache
            .borrow_mut()
            .add_instrument(make_btc_option(strike, OptionKind::Put));
    }

    let series_id = make_series_id();
    data_engine
        .borrow_mut()
        .execute(DataCommand::Subscribe(SubscribeCommand::OptionChain(
            SubscribeOptionChain::new(
                series_id,
                StrikeRange::AtmRelative {
                    strikes_above: 0,
                    strikes_below: 0,
                },
                None,
                UUID4::new(),
                UnixNanos::default(),
                Some(client_id),
                Some(venue),
                None,
            ),
        )));

    let request_id = option_chain_reference_price_request_id(&recorder);
    recorder.borrow_mut().clear();

    data_engine
        .borrow_mut()
        .response(DataResponse::OptionChainReferencePrice(
            OptionChainReferencePriceResponse::new(
                request_id,
                client_id,
                series_id,
                None,
                UnixNanos::default(),
                None,
            ),
        ));
    let sample_id = InstrumentId::from("BTC-20240101-45000.000-C.DERIBIT");
    assert!(recorder.borrow().iter().any(|cmd| {
        matches!(
            cmd,
            DataCommand::Subscribe(SubscribeCommand::OptionGreeks(cmd))
                if cmd.instrument_id == sample_id && cmd.client_id == Some(client_id)
        )
    }));
    recorder.borrow_mut().clear();

    data_engine
        .borrow_mut()
        .process_data(Data::OptionGreeks(make_option_chain_greeks(
            sample_id, 50000.0,
        )));

    let recorded = recorder.borrow();
    let quote_subscriptions = recorded
        .iter()
        .filter(|cmd| matches!(cmd, DataCommand::Subscribe(SubscribeCommand::Quotes(_))))
        .count();

    let greeks_subscriptions = recorded
        .iter()
        .filter(|cmd| {
            matches!(
                cmd,
                DataCommand::Subscribe(SubscribeCommand::OptionGreeks(_))
            )
        })
        .count();

    let greeks_unsubscriptions: Vec<InstrumentId> = recorded
        .iter()
        .filter_map(|cmd| match cmd {
            DataCommand::Unsubscribe(UnsubscribeCommand::OptionGreeks(cmd)) => {
                Some(cmd.instrument_id)
            }
            _ => None,
        })
        .collect();

    assert_eq!(quote_subscriptions, 2);
    assert_eq!(greeks_subscriptions, 2);
    assert_eq!(greeks_unsubscriptions, vec![sample_id]);
}

#[rstest]
fn test_option_chain_greeks_bootstrap_holds_subscription_ownership(
    clock: Rc<RefCell<VirtualClock>>,
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
    let sample_id = InstrumentId::from("BTC-20240101-50000.000-C.DERIBIT");
    let _ = cache
        .borrow_mut()
        .add_instrument(make_btc_option("50000.000", OptionKind::Call));

    let topic = switchboard::get_option_greeks_topic(sample_id);
    let (handler, _) = get_typed_message_saving_handler::<OptionGreeks>(None);
    msgbus::subscribe_option_greeks(topic.into(), handler.clone(), None);
    data_engine
        .borrow_mut()
        .execute(DataCommand::Subscribe(SubscribeCommand::OptionGreeks(
            SubscribeOptionGreeks::new(
                sample_id,
                Some(client_id),
                Some(venue),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ),
        )));
    recorder.borrow_mut().clear();

    let series_id = make_series_id();
    data_engine
        .borrow_mut()
        .execute(make_subscribe_option_chain_atm(series_id, client_id, venue));
    let request_id = option_chain_reference_price_request_id(&recorder);
    data_engine
        .borrow_mut()
        .response(DataResponse::OptionChainReferencePrice(
            OptionChainReferencePriceResponse::new(
                request_id,
                client_id,
                series_id,
                None,
                UnixNanos::default(),
                None,
            ),
        ));
    recorder.borrow_mut().clear();

    msgbus::unsubscribe_option_greeks(topic.into(), &handler);
    data_engine
        .borrow_mut()
        .execute(DataCommand::Unsubscribe(UnsubscribeCommand::OptionGreeks(
            UnsubscribeOptionGreeks::new(
                sample_id,
                Some(client_id),
                Some(venue),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ),
        )));

    assert!(recorder.borrow().is_empty());
    data_engine
        .borrow_mut()
        .process_data(Data::OptionGreeks(make_option_chain_greeks(
            sample_id, 50000.0,
        )));
    assert!(data_engine.borrow().has_option_chain_manager(&series_id));
    assert!(
        recorder
            .borrow()
            .iter()
            .all(|cmd| !matches!(cmd, DataCommand::Unsubscribe(_)))
    );
}

#[rstest]
fn test_unsubscribe_option_chain_preserves_user_owned_bootstrap_greeks(
    clock: Rc<RefCell<VirtualClock>>,
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
    let sample_id = InstrumentId::from("BTC-20240101-50000.000-C.DERIBIT");
    let _ = cache
        .borrow_mut()
        .add_instrument(make_btc_option("50000.000", OptionKind::Call));

    let topic = switchboard::get_option_greeks_topic(sample_id);
    let (handler, _) = get_typed_message_saving_handler::<OptionGreeks>(None);
    msgbus::subscribe_option_greeks(topic.into(), handler, None);
    data_engine
        .borrow_mut()
        .execute(DataCommand::Subscribe(SubscribeCommand::OptionGreeks(
            SubscribeOptionGreeks::new(
                sample_id,
                Some(client_id),
                Some(venue),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ),
        )));

    let series_id = make_series_id();
    data_engine
        .borrow_mut()
        .execute(make_subscribe_option_chain_atm(series_id, client_id, venue));
    let request_id = option_chain_reference_price_request_id(&recorder);
    data_engine
        .borrow_mut()
        .response(DataResponse::OptionChainReferencePrice(
            OptionChainReferencePriceResponse::new(
                request_id,
                client_id,
                series_id,
                None,
                UnixNanos::default(),
                None,
            ),
        ));
    recorder.borrow_mut().clear();

    data_engine
        .borrow_mut()
        .execute(make_unsubscribe_option_chain(
            series_id,
            Some(client_id),
            Some(venue),
        ));
    assert!(recorder.borrow().is_empty());
    let sample_is_subscribed = data_engine
        .borrow_mut()
        .get_client(Some(&client_id), Some(&venue))
        .is_some_and(|client| client.subscriptions_option_greeks.contains(&sample_id));
    assert!(sample_is_subscribed);
}

#[rstest]
#[case::data_owned(OptionGreeksDispatch::DataOwned)]
#[case::typed_any(OptionGreeksDispatch::TypedAny)]
#[case::data_borrowed(OptionGreeksDispatch::DataBorrowed)]
fn test_option_chain_deferred_bootstrap_from_greeks_keeps_bootstrap_event(
    #[case] dispatch: OptionGreeksDispatch,
    clock: Rc<RefCell<VirtualClock>>,
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

    let strikes = ["45000.000", "50000.000", "55000.000"];
    for strike in &strikes {
        let call = make_btc_option(strike, OptionKind::Call);
        let put = make_btc_option(strike, OptionKind::Put);
        let _ = cache.borrow_mut().add_instrument(call);
        let _ = cache.borrow_mut().add_instrument(put);
    }

    let series_id = make_series_id();
    let topic = switchboard::get_option_chain_topic(series_id);
    let (handler, saver) = get_typed_message_saving_handler::<OptionChainSlice>(None);
    msgbus::subscribe_option_chain(topic.into(), handler, None);

    let cmd = DataCommand::Subscribe(SubscribeCommand::OptionChain(SubscribeOptionChain::new(
        series_id,
        StrikeRange::AtmRelative {
            strikes_above: 1,
            strikes_below: 1,
        },
        None,
        UUID4::new(),
        UnixNanos::default(),
        Some(client_id),
        Some(venue),
        None,
    )));

    data_engine.borrow_mut().execute(cmd);

    let request_id = recorder
        .borrow()
        .iter()
        .find_map(|cmd| match cmd {
            DataCommand::Request(RequestCommand::OptionChainReferencePrice(req)) => {
                Some(req.request_id)
            }
            _ => None,
        })
        .expect("reference price request should be recorded");

    data_engine
        .borrow_mut()
        .response(DataResponse::OptionChainReferencePrice(
            OptionChainReferencePriceResponse::new(
                request_id,
                client_id,
                series_id,
                None,
                UnixNanos::default(),
                None,
            ),
        ));
    assert!(data_engine.borrow().has_option_chain_manager(&series_id));
    assert_eq!(data_engine.borrow().pending_option_chain_request_count(), 0);

    let bootstrap_instrument_id = InstrumentId::from("BTC-20240101-45000.000-C.DERIBIT");

    let bootstrap_subscriptions: Vec<SubscribeOptionGreeks> = recorder
        .borrow()
        .iter()
        .filter_map(|cmd| match cmd {
            DataCommand::Subscribe(SubscribeCommand::OptionGreeks(cmd)) => Some(cmd.clone()),
            _ => None,
        })
        .collect();

    assert_eq!(bootstrap_subscriptions.len(), 1);
    assert_eq!(
        bootstrap_subscriptions[0].instrument_id,
        bootstrap_instrument_id
    );
    assert_eq!(bootstrap_subscriptions[0].client_id, Some(client_id));

    recorder.borrow_mut().clear();

    let call_id = InstrumentId::from("BTC-20240101-50000.000-C.DERIBIT");
    let greeks = make_option_chain_greeks(call_id, 50000.0);

    match dispatch {
        OptionGreeksDispatch::DataOwned => data_engine
            .borrow_mut()
            .process_data(Data::OptionGreeks(greeks)),
        OptionGreeksDispatch::TypedAny => data_engine.borrow_mut().process(&greeks),
        OptionGreeksDispatch::DataBorrowed => data_engine
            .borrow_mut()
            .process_data_ref(DataRef::OptionGreeks(&greeks)),
    }

    let recorded = recorder.borrow();
    let quote_subs = recorded
        .iter()
        .filter(|cmd| matches!(cmd, DataCommand::Subscribe(SubscribeCommand::Quotes(_))))
        .count();

    let greeks_subs = recorded
        .iter()
        .filter(|cmd| {
            matches!(
                cmd,
                DataCommand::Subscribe(SubscribeCommand::OptionGreeks(_))
            )
        })
        .count();

    let status_subs = recorded
        .iter()
        .filter(|cmd| {
            matches!(
                cmd,
                DataCommand::Subscribe(SubscribeCommand::InstrumentStatus(_))
            )
        })
        .count();

    assert_eq!(quote_subs, 6);
    assert_eq!(greeks_subs, 5);
    assert_eq!(status_subs, 6);
    drop(recorded);

    assert!(saver.get_messages().is_empty());

    let quote = QuoteTick::new(
        call_id,
        Price::from("100.00"),
        Price::from("101.00"),
        Quantity::from("1.0"),
        Quantity::from("1.0"),
        UnixNanos::from(2u64),
        UnixNanos::from(2u64),
    );
    data_engine.borrow_mut().process_data(Data::Quote(quote));

    let messages = saver.get_messages();
    assert_eq!(messages.len(), 1);
    let slice = messages.last().expect("raw quote should publish a slice");
    let strike = Price::from("50000.000");
    let call = slice
        .get_call(&strike)
        .expect("quoted call strike should be present");
    let greeks = call
        .greeks
        .as_ref()
        .expect("bootstrap Greeks should be retained for the first quote");

    assert_eq!(greeks.instrument_id, call_id);
    assert_eq!(greeks.delta, 0.55);
}

#[rstest]
fn test_option_chain_unsubscribe_releases_active_bootstrap_sample(
    clock: Rc<RefCell<VirtualClock>>,
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

    for strike in ["45000.000", "50000.000", "55000.000"] {
        let _ = cache
            .borrow_mut()
            .add_instrument(make_btc_option(strike, OptionKind::Call));
        let _ = cache
            .borrow_mut()
            .add_instrument(make_btc_option(strike, OptionKind::Put));
    }

    let series_id = make_series_id();
    let topic = switchboard::get_option_chain_topic(series_id);
    let (handler, _saver) = get_typed_message_saving_handler::<OptionChainSlice>(None);
    msgbus::subscribe_option_chain(topic.into(), handler.clone(), None);
    data_engine
        .borrow_mut()
        .execute(DataCommand::Subscribe(SubscribeCommand::OptionChain(
            SubscribeOptionChain::new(
                series_id,
                StrikeRange::AtmRelative {
                    strikes_above: 1,
                    strikes_below: 1,
                },
                None,
                UUID4::new(),
                UnixNanos::default(),
                Some(client_id),
                Some(venue),
                None,
            ),
        )));

    let request_id = recorder
        .borrow()
        .iter()
        .find_map(|cmd| match cmd {
            DataCommand::Request(RequestCommand::OptionChainReferencePrice(req)) => {
                Some(req.request_id)
            }
            _ => None,
        })
        .expect("reference price request should be recorded");

    data_engine
        .borrow_mut()
        .response(DataResponse::OptionChainReferencePrice(
            OptionChainReferencePriceResponse::new(
                request_id,
                client_id,
                series_id,
                None,
                UnixNanos::default(),
                None,
            ),
        ));

    // The bootstrap sample (45000-C) lands inside the active window
    let sample_id = InstrumentId::from("BTC-20240101-45000.000-C.DERIBIT");
    let call_id = InstrumentId::from("BTC-20240101-50000.000-C.DERIBIT");
    data_engine
        .borrow_mut()
        .process_data(Data::OptionGreeks(make_option_chain_greeks(
            call_id, 50000.0,
        )));

    let greeks_unsubscribes = |recorder: &Rc<RefCell<Vec<DataCommand>>>| -> Vec<InstrumentId> {
        recorder
            .borrow()
            .iter()
            .filter_map(|cmd| match cmd {
                DataCommand::Unsubscribe(UnsubscribeCommand::OptionGreeks(cmd)) => {
                    Some(cmd.instrument_id)
                }
                _ => None,
            })
            .collect()
    };

    assert!(greeks_unsubscribes(&recorder).is_empty());

    msgbus::unsubscribe_option_chain(topic.into(), &handler);
    recorder.borrow_mut().clear();
    data_engine
        .borrow_mut()
        .execute(make_unsubscribe_option_chain(
            series_id,
            Some(client_id),
            Some(venue),
        ));

    assert!(greeks_unsubscribes(&recorder).contains(&sample_id));
    assert!(
        data_engine.borrow().get_clients()[0]
            .subscriptions_option_greeks
            .is_empty()
    );
}

#[rstest]
fn test_process_pipeline_instrument_status_skips_option_chain_expiry(
    clock: Rc<RefCell<VirtualClock>>,
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

    recorder.borrow_mut().clear();

    // Drive a Close status through the pipeline; live path would expire the
    // instrument and emit wire-level unsubscribes, pipeline must not.
    let status = InstrumentStatus::new(
        call_id,
        MarketStatusAction::Close,
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
        .process_pipeline(Data::InstrumentStatus(status));

    let unsubs: Vec<_> = recorder
        .borrow()
        .iter()
        .filter(|cmd| matches!(cmd, DataCommand::Unsubscribe(_)))
        .cloned()
        .collect();
    assert!(
        unsubs.is_empty(),
        "pipeline instrument status must not trigger option chain expiry (got {unsubs:?})",
    );
    assert!(
        data_engine.borrow().has_option_chain_manager(&series_id),
        "option chain manager must remain intact after pipeline status",
    );
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

fn make_option_chain_greeks(instrument_id: InstrumentId, underlying_price: f64) -> OptionGreeks {
    OptionGreeks {
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
        underlying_price: Some(underlying_price),
        open_interest: Some(1000.0),
        ts_event: UnixNanos::from(1u64),
        ts_init: UnixNanos::from(1u64),
    }
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

fn make_subscribe_option_chain_atm(
    series_id: OptionSeriesId,
    client_id: ClientId,
    venue: Venue,
) -> DataCommand {
    DataCommand::Subscribe(SubscribeCommand::OptionChain(SubscribeOptionChain::new(
        series_id,
        StrikeRange::AtmRelative {
            strikes_above: 1,
            strikes_below: 1,
        },
        None,
        UUID4::new(),
        UnixNanos::default(),
        Some(client_id),
        Some(venue),
        None,
    )))
}

fn option_chain_reference_price_request_id(recorder: &Rc<RefCell<Vec<DataCommand>>>) -> UUID4 {
    recorder
        .borrow()
        .iter()
        .find_map(|cmd| match cmd {
            DataCommand::Request(RequestCommand::OptionChainReferencePrice(request)) => {
                Some(request.request_id)
            }
            _ => None,
        })
        .expect("reference price request should be recorded")
}

#[derive(Clone, Copy)]
enum OptionGreeksDispatch {
    DataOwned,
    TypedAny,
    DataBorrowed,
}

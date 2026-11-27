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

use std::sync::Arc;

use nautilus_common::{
    actor::{
        DataActor, DataActorCore,
        data_actor::DataActorConfig,
        registry::{deregister_actor, get_actor_unchecked, register_actor},
    },
    component::Component,
    messages::data::DataRequestFailed,
    nautilus_actor,
    runner::{SyncDataCommandSender, set_data_cmd_sender},
};
use nautilus_model::{
    enums::GreeksConvention::{BlackScholes, PriceAdjusted},
    identifiers::ActorId,
};

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
    let data_engine = make_option_chain_engine(Rc::clone(&clock), Rc::clone(&cache));

    let sim_recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        Rc::clone(&clock),
        Rc::clone(&cache),
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
        Rc::clone(&cache),
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
fn test_external_option_chain_releases_each_owner(
    #[values(false, true)] stopped: bool,
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
    let first_params = Params::new([("owner".to_string(), serde_json::json!(1))].into());
    let second_params = Params::new([("owner".to_string(), serde_json::json!(2))].into());

    let subscribe = |command_id, strike, params| {
        SubscribeCommand::OptionChain(
            SubscribeOptionChain::new(
                series_id,
                StrikeRange::Fixed(vec![Price::from(strike)]),
                Some(1_000),
                command_id,
                UnixNanos::default(),
                Some(client_id),
                Some(venue),
                Some(params),
            )
            .unwrap(),
        )
    };

    let (subscribe_handler, subscribe_saver) = get_any_saving_handler::<SubscribeCommand>(None);
    let (unsubscribe_handler, unsubscribe_saver) =
        get_any_saving_handler::<UnsubscribeCommand>(None);
    msgbus::subscribe_any(topic.as_str().into(), subscribe_handler, None);
    msgbus::subscribe_any(topic.as_str().into(), unsubscribe_handler, None);

    if stopped {
        data_engine.stop();
    }

    let first = subscribe(UUID4::new(), "50000", first_params.clone());
    let second = subscribe(UUID4::new(), "51000", second_params.clone());
    data_engine.execute(DataCommand::Subscribe(first.clone()));
    data_engine.execute(DataCommand::Subscribe(second.clone()));
    assert_eq!(
        subscribe_saver.get_messages().len(),
        if stopped { 0 } else { 2 }
    );

    for (subscribe, params, ts) in [(first, first_params, 1), (second, second_params, 2)] {
        let owner_id = subscribe.command_id();
        let unsubscribe = subscribe.into_unsubscribe(UUID4::new(), UnixNanos::from(ts), None);
        data_engine.execute(DataCommand::Unsubscribe(unsubscribe.clone()));
        data_engine.execute(DataCommand::Unsubscribe(unsubscribe));
        let commands = unsubscribe_saver.get_messages();
        assert_eq!(commands.len(), ts as usize);

        let UnsubscribeCommand::OptionChain(command) = commands.last().unwrap() else {
            panic!("expected external option chain unsubscribe");
        };

        assert_eq!(command.series_id, series_id);
        assert_eq!(command.client_id, Some(client_id));
        assert_eq!(command.venue, Some(venue));
        assert_eq!(command.ts_init, UnixNanos::from(ts));
        assert_eq!(command.correlation_id, Some(owner_id));
        assert_eq!(command.params.as_ref(), Some(&params));
    }
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
    let initial_params = Params::new([("route".to_string(), serde_json::json!(1))].into());
    let edited_params = Params::new([("route".to_string(), serde_json::json!(2))].into());
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
        )
        .unwrap(),
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
    )
    .unwrap();
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
#[case("")]
#[case("   ")]
fn test_option_chain_update_skips_invalid_deserialized_underlying(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    #[case] underlying: &str,
) {
    let _ = msgbus::get_message_bus();
    let data_engine = make_option_chain_engine(clock, Rc::clone(&cache));
    let option = nautilus_model::instruments::stubs::option_contract_appl();
    let mut value = serde_json::to_value(option).unwrap();
    value["underlying"] = json!(underlying);
    let option: OptionContract = serde_json::from_value(value).unwrap();
    let instrument = InstrumentAny::OptionContract(option);
    data_engine.borrow_mut().process(&instrument);

    assert_eq!(instrument.underlying(), Some(Ustr::from(underlying)));
    assert_eq!(
        cache
            .borrow()
            .instrument(&instrument.id())
            .unwrap()
            .underlying(),
        Some(Ustr::from(underlying)),
    );
}

#[rstest]
fn test_subscribe_option_chain_fixed_range_creates_manager(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let _ = msgbus::get_message_bus();
    let data_engine = make_option_chain_engine(Rc::clone(&clock), Rc::clone(&cache));

    let client_id = ClientId::new("DERIBIT");
    let venue = Venue::new("DERIBIT");
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));

    register_mock_client(
        clock,
        Rc::clone(&cache),
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
#[case(false, false)]
#[case(false, true)]
#[case(true, false)]
#[case(true, true)]
fn test_subscribe_option_chain_rejects_explicit_reference(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    #[case] dynamic: bool,
    #[case] subscribed_derived: bool,
) {
    let _ = msgbus::get_message_bus();
    let data_engine = make_option_chain_engine(Rc::clone(&clock), Rc::clone(&cache));
    let client_id = ClientId::new("DERIBIT");
    let venue = Venue::new("DERIBIT");
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));
    register_mock_client(
        Rc::clone(&clock),
        Rc::clone(&cache),
        client_id,
        venue,
        Some(venue),
        &recorder,
        &mut data_engine.borrow_mut(),
    );
    cache
        .borrow_mut()
        .add_instrument(make_btc_option("50000.000", OptionKind::Call))
        .unwrap();
    let derived = make_series_id();

    if subscribed_derived {
        data_engine
            .borrow_mut()
            .execute(make_subscribe_option_chain(
                derived,
                vec![Price::from("50000.000")],
                Some(client_id),
                Some(venue),
            ));
    }

    recorder.borrow_mut().clear();
    let timers: Vec<String> = clock
        .borrow()
        .timer_names()
        .into_iter()
        .map(String::from)
        .collect();

    let explicit = OptionSeriesId::new(
        derived.venue,
        derived.underlying,
        derived.settlement_currency,
        derived.expiration_ns,
        InstrumentId::from("BTCUSDT.BINANCE"),
    );

    let command = if dynamic {
        make_subscribe_option_chain_atm(explicit, client_id, venue)
    } else {
        make_subscribe_option_chain(
            explicit,
            vec![Price::from("50000.000")],
            Some(client_id),
            Some(venue),
        )
    };

    data_engine.borrow_mut().execute(command);

    assert!(!data_engine.borrow().has_option_chain_manager(&explicit));
    assert_eq!(
        data_engine.borrow().has_option_chain_manager(&derived),
        subscribed_derived,
    );
    assert!(recorder.borrow().is_empty());
    assert_eq!(
        clock
            .borrow()
            .timer_names()
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>(),
        timers,
    );
}

#[rstest]
#[case::local(false)]
#[case::external(true)]
fn test_option_chain_rejects_invalid_greeks_convention_at_dispatch(
    #[case] external: bool,
    _stub_msgbus: Rc<RefCell<MessageBus>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let series_id = make_series_id();
    let client_id = ClientId::from("DERIBIT");

    let config = DataEngineConfig {
        external_clients: external.then_some(vec![client_id]),
        ..Default::default()
    };

    let mut engine = DataEngine::new(clock, cache, Some(config));
    let (handler, saver) = get_any_saving_handler::<SubscribeCommand>(None);
    let topic = format!("commands.data.{client_id}");
    msgbus::subscribe_any(topic.as_str().into(), handler, None);

    let mut command = SubscribeOptionChain::new(
        series_id,
        StrikeRange::Fixed(vec![Price::from("50000.000")]),
        None,
        UUID4::new(),
        UnixNanos::default(),
        Some(client_id),
        Some(series_id.venue),
        None,
    )
    .unwrap();
    command.params = Some(Params::new(
        [("greeks_convention".to_string(), json!("INDEX_PRICE"))].into(),
    ));
    let error = engine
        .execute_subscribe(SubscribeCommand::OptionChain(command))
        .unwrap_err();

    assert_eq!(
        error.to_string(),
        r#"invalid Greeks convention for 'greeks_convention', was "INDEX_PRICE""#,
    );
    assert!(!engine.has_option_chain_manager(&series_id));
    assert!(saver.get_messages().is_empty());
}

#[rstest]
fn test_subscribe_option_chain_filters_by_underlying(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let _ = msgbus::get_message_bus();
    let data_engine = make_option_chain_engine(Rc::clone(&clock), Rc::clone(&cache));

    let client_id = ClientId::new("DERIBIT");
    let venue = Venue::new("DERIBIT");
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));

    register_mock_client(
        clock,
        Rc::clone(&cache),
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
        UnixNanos::from(1_704_067_200_000_000_000_u64),
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
fn test_stopped_option_chain_keeps_local_wiring_without_physical_subscriptions(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let data_engine = make_option_chain_engine(Rc::clone(&clock), Rc::clone(&cache));
    let client_id = ClientId::new("DERIBIT");
    let venue = Venue::new("DERIBIT");
    let recorder = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        Rc::clone(&cache),
        client_id,
        venue,
        Some(venue),
        &recorder,
        &mut data_engine.borrow_mut(),
    );
    let call = make_btc_option("50000.000", OptionKind::Call);
    let call_id = call.id();
    cache.borrow_mut().add_instrument(call).unwrap();
    let series_id = make_series_id();

    data_engine.borrow_mut().stop();
    data_engine
        .borrow_mut()
        .execute(make_subscribe_option_chain(
            series_id,
            vec![Price::from("50000.000")],
            Some(client_id),
            Some(venue),
        ));
    let put = make_btc_option("50000.000", OptionKind::Put);
    let put_id = put.id();
    data_engine.borrow_mut().process(&put);

    assert!(data_engine.borrow().has_option_chain_manager(&series_id));

    for instrument_id in [call_id, put_id] {
        let topic = switchboard::get_quotes_topic(instrument_id);
        assert_eq!(msgbus::exact_subscriber_count_quotes(topic), 1);
    }

    assert!(recorder.borrow().is_empty());

    data_engine
        .borrow_mut()
        .execute(make_unsubscribe_option_chain(
            series_id,
            Some(client_id),
            Some(venue),
        ));
    assert!(!data_engine.borrow().has_option_chain_manager(&series_id));

    for instrument_id in [call_id, put_id] {
        let topic = switchboard::get_quotes_topic(instrument_id);
        assert_eq!(msgbus::exact_subscriber_count_quotes(topic), 0);
    }

    assert!(recorder.borrow().is_empty());
}

#[rstest]
fn test_option_chain_new_instrument_uses_subscription_client(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let _ = msgbus::get_message_bus();
    let data_engine = make_option_chain_engine(Rc::clone(&clock), Rc::clone(&cache));
    let explicit_client_id = ClientId::new("DERIBIT-EXPLICIT");
    let routed_client_id = ClientId::new("DERIBIT-ROUTED");
    let venue = Venue::new("DERIBIT");
    let explicit_recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));
    let routed_recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));
    register_mock_client(
        Rc::clone(&clock),
        Rc::clone(&cache),
        explicit_client_id,
        venue,
        None,
        &explicit_recorder,
        &mut data_engine.borrow_mut(),
    );
    register_mock_client(
        clock,
        Rc::clone(&cache),
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
        DataCommand::Subscribe(SubscribeCommand::Quotes(cmd)) if cmd.instrument_ids == [put_id]
    )));
    assert!(recorded.iter().any(|cmd| matches!(
        cmd,
        DataCommand::Subscribe(SubscribeCommand::OptionGreeks(cmd))
            if cmd.single_instrument_id().unwrap() == put_id
    )));
    assert!(recorded.iter().any(|cmd| matches!(
        cmd,
        DataCommand::Subscribe(SubscribeCommand::InstrumentStatus(cmd))
            if cmd.single_instrument_id().unwrap() == put_id
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
            if cmd.instrument_ids == [put_id]
    )));
    assert!(recorded.iter().any(|cmd| matches!(
        cmd,
        DataCommand::Unsubscribe(UnsubscribeCommand::OptionGreeks(cmd))
            if cmd.single_instrument_id().unwrap() == put_id
    )));
    assert!(recorded.iter().any(|cmd| matches!(
        cmd,
        DataCommand::Unsubscribe(UnsubscribeCommand::InstrumentStatus(cmd))
            if cmd.single_instrument_id().unwrap() == put_id
    )));
    assert!(routed_recorder.borrow().is_empty());
}

#[rstest]
fn test_option_chain_out_of_range_listing_is_not_subscribed(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let _ = msgbus::get_message_bus();
    let data_engine = make_option_chain_engine(Rc::clone(&clock), Rc::clone(&cache));
    let client_id = ClientId::new("DERIBIT");
    let venue = Venue::new("DERIBIT");
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));
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
    let data_engine = make_option_chain_engine(Rc::clone(&clock), Rc::clone(&cache));

    let client_id = ClientId::new("DERIBIT");
    let venue = Venue::new("DERIBIT");
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));

    register_mock_client(
        clock,
        Rc::clone(&cache),
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
    let engine = make_option_chain_engine(Rc::clone(&clock), Rc::clone(&cache));
    let client_id = ClientId::from("DERIBIT");
    let venue = Venue::from("DERIBIT");
    let recorder = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        Rc::clone(&cache),
        client_id,
        venue,
        Some(venue),
        &recorder,
        &mut engine.borrow_mut(),
    );
    cache
        .borrow_mut()
        .add_instrument(make_btc_option("50000.000", OptionKind::Call))
        .unwrap();
    let series = make_series_id();
    let first = make_subscribe_option_chain(
        series,
        vec![Price::from("50000.000")],
        Some(client_id),
        Some(venue),
    );
    let second = make_subscribe_option_chain(
        series,
        vec![Price::from("50000.000")],
        Some(client_id),
        Some(venue),
    );
    engine.borrow_mut().execute(first.clone());
    engine.borrow_mut().execute(second.clone());
    recorder.borrow_mut().clear();
    engine.borrow_mut().execute(make_unsubscribe_option_chain(
        series,
        Some(client_id),
        Some(venue),
    ));
    assert!(recorder.borrow().is_empty());
    assert_eq!(engine.borrow().clock().borrow().timer_count(), 2);
    engine
        .borrow_mut()
        .execute(option_chain_unsubscribe(&first));
    engine
        .borrow_mut()
        .execute(option_chain_unsubscribe(&first));
    assert!(engine.borrow().has_option_chain_manager(&series));
    assert!(recorder.borrow().is_empty());
    engine
        .borrow_mut()
        .execute(option_chain_unsubscribe(&second));
    assert!(!engine.borrow().has_option_chain_manager(&series));
    assert_eq!(recorder.borrow().len(), 3);
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
    let engine = make_option_chain_engine(Rc::clone(&clock), Rc::clone(&cache));
    let client_id = ClientId::new("DERIBIT");
    let venue = Venue::new("DERIBIT");
    let recorder = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        Rc::clone(&clock),
        Rc::clone(&cache),
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
            )
            .unwrap(),
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
        engine.borrow_mut().execute(make_unsubscribe_option_chain(
            series_id,
            Some(client_id),
            Some(venue),
        ));
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
        DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(command)) if command.instrument_ids == [next_id]
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
    let engine = make_option_chain_engine(Rc::clone(&clock), Rc::clone(&cache));
    let client_id = ClientId::from("DERIBIT");
    let venue = Venue::from("DERIBIT");
    let recorder = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        Rc::clone(&cache),
        client_id,
        venue,
        Some(venue),
        &recorder,
        &mut engine.borrow_mut(),
    );
    cache
        .borrow_mut()
        .add_instrument(make_btc_option("50000.000", OptionKind::Call))
        .unwrap();
    let series = make_series_id();
    let first = make_subscribe_option_chain_atm(series, client_id, venue);
    let second = make_subscribe_option_chain_atm(series, client_id, venue);
    engine.borrow_mut().execute(first.clone());
    let first_request = option_chain_reference_price_request_id(&recorder);
    recorder.borrow_mut().clear();
    engine.borrow_mut().execute(second.clone());
    let second_request = option_chain_reference_price_request_id(&recorder);
    assert_eq!(engine.borrow().pending_option_chain_request_count(), 2);
    engine
        .borrow_mut()
        .execute(option_chain_unsubscribe(&first));
    engine
        .borrow_mut()
        .response(DataResponse::OptionChainReferencePrice(
            OptionChainReferencePriceResponse::new(
                first_request,
                client_id,
                series,
                Some(Price::from("50000.000")),
                UnixNanos::from(1),
                None,
            ),
        ));
    assert_eq!(engine.borrow().pending_option_chain_request_count(), 1);
    assert!(!engine.borrow().has_option_chain_manager(&series));
    engine
        .borrow_mut()
        .response(DataResponse::OptionChainReferencePrice(
            OptionChainReferencePriceResponse::new(
                second_request,
                client_id,
                series,
                Some(Price::from("50000.000")),
                UnixNanos::from(2),
                None,
            ),
        ));
    assert_eq!(engine.borrow().pending_option_chain_request_count(), 0);
    assert!(engine.borrow().has_option_chain_manager(&series));
    engine
        .borrow_mut()
        .execute(option_chain_unsubscribe(&second));
    assert!(!engine.borrow().has_option_chain_manager(&series));
}

#[rstest]
fn test_option_chain_owners_keep_separate_clients(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let _ = msgbus::get_message_bus();
    let data_engine = make_option_chain_engine(Rc::clone(&clock), Rc::clone(&cache));
    let venue = Venue::new("DERIBIT");
    let first_client_id = ClientId::new("DERIBIT-FIRST");
    let second_client_id = ClientId::new("DERIBIT-SECOND");
    let first_recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));
    let second_recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));
    register_mock_client(
        Rc::clone(&clock),
        Rc::clone(&cache),
        first_client_id,
        venue,
        None,
        &first_recorder,
        &mut data_engine.borrow_mut(),
    );
    register_mock_client(
        clock,
        Rc::clone(&cache),
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

    let first = make_subscribe_option_chain(
        series_id,
        strikes.clone(),
        Some(first_client_id),
        Some(venue),
    );
    let second =
        make_subscribe_option_chain(series_id, strikes, Some(second_client_id), Some(venue));
    data_engine.borrow_mut().execute(first.clone());
    data_engine.borrow_mut().execute(second.clone());
    assert!(
        !first_recorder
            .borrow()
            .iter()
            .any(|command| matches!(command, DataCommand::Unsubscribe(_)))
    );
    assert!(
        !second_recorder
            .borrow()
            .iter()
            .any(|command| matches!(command, DataCommand::Unsubscribe(_)))
    );
    data_engine
        .borrow_mut()
        .execute(option_chain_unsubscribe(&first));
    assert!(first_recorder.borrow().iter().any(|command| matches!(command,
        DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(command)) if command.client_id == Some(first_client_id))));
    assert!(
        !second_recorder
            .borrow()
            .iter()
            .any(|command| matches!(command, DataCommand::Unsubscribe(_)))
    );
    assert!(data_engine.borrow().has_option_chain_manager(&series_id));
    data_engine
        .borrow_mut()
        .execute(option_chain_unsubscribe(&second));
    assert!(second_recorder.borrow().iter().any(|command| matches!(command,
        DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(command)) if command.client_id == Some(second_client_id))));
    assert!(!data_engine.borrow().has_option_chain_manager(&series_id));
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
fn test_subscribe_option_chain_replay_keeps_manager(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let _ = msgbus::get_message_bus();
    let data_engine = make_option_chain_engine(Rc::clone(&clock), Rc::clone(&cache));

    let client_id = ClientId::new("DERIBIT");
    let venue = Venue::new("DERIBIT");
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));

    register_mock_client(
        clock,
        Rc::clone(&cache),
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
    let command = make_subscribe_option_chain(series_id, strikes, Some(client_id), Some(venue));
    data_engine.borrow_mut().execute(command.clone());
    data_engine.borrow_mut().execute(command.clone());
    assert_eq!(recorder.borrow().len(), 3);
    assert_eq!(data_engine.borrow().clock().borrow().timer_count(), 1);
    data_engine
        .borrow_mut()
        .execute(option_chain_unsubscribe(&command));
    assert!(!data_engine.borrow().has_option_chain_manager(&series_id));
    assert_eq!(recorder.borrow().len(), 6);
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
    let data_engine = make_option_chain_engine(Rc::clone(&clock), Rc::clone(&cache));

    let client_id = ClientId::new("DERIBIT");
    let venue = Venue::new("DERIBIT");
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));

    register_mock_client(
        clock,
        Rc::clone(&cache),
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
    let data_engine = make_option_chain_engine(Rc::clone(&clock), Rc::clone(&cache));

    let client_id = ClientId::new("DERIBIT");
    let venue = Venue::new("DERIBIT");
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));

    register_mock_client(
        clock,
        Rc::clone(&cache),
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
    let data_engine = make_option_chain_engine(Rc::clone(&clock), Rc::clone(&cache));

    let client_id = ClientId::new("DERIBIT");
    let venue = Venue::new("DERIBIT");
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));

    register_mock_client(
        clock,
        Rc::clone(&cache),
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

    let cmd = DataCommand::Subscribe(SubscribeCommand::OptionChain(
        SubscribeOptionChain::new(
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
        )
        .unwrap(),
    ));

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
    let data_engine = make_option_chain_engine(Rc::clone(&clock), Rc::clone(&cache));
    let client_id = ClientId::new("DERIBIT");
    let venue = Venue::new("DERIBIT");
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));
    register_mock_client(
        Rc::clone(&clock),
        Rc::clone(&cache),
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
            )
            .unwrap(),
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
#[case::raw(None, false)]
#[case::raw_expiry(None, true)]
#[case::snapshot(Some(1_000), false)]
fn test_option_chain_delta_discovers_and_releases_candidates(
    #[case] interval: Option<u64>,
    #[case] market_data_expiry: bool,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let fixture = OptionChainFixture::new(clock, &cache);
    let series_id = make_series_id();
    if interval.is_some() {
        let start = series_id.expiration_ns - DurationNanos::from_secs(8);
        fixture.clock.borrow().set_time(start);
    }

    let strikes: Vec<_> = (40..=64).map(|index| format!("{index}000.000")).collect();

    let ids: Vec<_> = strikes
        .iter()
        .map(|strike| {
            [OptionKind::Call, OptionKind::Put].map(|kind| {
                let instrument = make_btc_option(strike, kind);
                let id = instrument.id();
                cache.borrow_mut().add_instrument(instrument).unwrap();
                id
            })
        })
        .collect();

    fixture.subscribe(0, StrikeRange::delta(0.25, 0.04).unwrap(), interval);
    fixture.resolve_reference_price(Price::from("50000.000"));

    let assert_subscriptions = |expected: &[[InstrumentId; 2]]| {
        let mut expected: Vec<_> = expected.iter().flatten().copied().collect();
        expected.sort_unstable();
        let engine = fixture.engine.borrow();
        let mut quotes = engine.subscribed_quotes();
        quotes.sort_unstable();
        let mut statuses = engine.subscribed_instrument_status();
        statuses.sort_unstable();
        assert_eq!(quotes, expected);
        assert_eq!(statuses, expected);

        for id in ids.iter().flatten() {
            let subscribers = usize::from(expected.contains(id));
            assert_eq!(
                msgbus::exact_subscriber_count_quotes(switchboard::get_quotes_topic(*id)),
                subscribers
            );
            assert_eq!(
                msgbus::exact_subscriber_count_option_greeks(switchboard::get_option_greeks_topic(
                    *id
                )),
                subscribers
            );
        }
    };

    assert_subscriptions(&ids[..21]);

    let feed = |id, delta, forward, ts| {
        let quote = make_quote(id, "101.000", "103.000", ts);
        let mut greeks = make_option_chain_greeks(id, forward);
        greeks.greeks.delta = delta;
        greeks.ts_event = UnixNanos::from(ts);
        let mut engine = fixture.engine.borrow_mut();
        engine.process_data(Data::Quote(quote));
        engine.process_data(Data::OptionGreeks(greeks));
    };

    let assert_slice = |expected: &[&str]| {
        if interval.is_some() {
            advance_clock_and_dispatch(&fixture.clock, 2_000_000_000);
        }

        let actor = get_actor_unchecked::<OptionChainActor>(&fixture.actors[0]);
        let slice = actor.slices.last().unwrap();
        let expected: Vec<_> = expected.iter().map(|strike| Price::from(*strike)).collect();
        assert_eq!(slice.calls.keys().copied().collect::<Vec<_>>(), expected);
        assert_eq!(slice.puts.keys().copied().collect::<Vec<_>>(), expected);
    };

    for (index, &[call, put]) in ids.iter().enumerate() {
        let (call_delta, put_delta) = match index {
            1 => (0.60, -0.23),
            2 => (0.60, -0.27),
            3 => (0.60, -0.20),
            4 => (0.60, -0.31),
            17 => (0.23, -0.60),
            18 => (0.27, -0.60),
            19 => (0.20, -0.60),
            20 => (0.31, -0.60),
            _ => (0.60, -0.60),
        };

        feed(call, call_delta, 50000.0, 10);
        feed(put, put_delta, 50000.0, 11);
    }

    assert_slice(&["41000.000", "42000.000", "57000.000", "58000.000"]);
    assert_subscriptions(&ids[..21]);

    feed(ids[10][0], 0.60, 54000.0, 12);
    assert_slice(&["57000.000", "58000.000"]);
    feed(ids[21][0], 0.25, 54000.0, 13);
    feed(ids[21][1], -0.60, 54000.0, 14);
    feed(ids[1][1], -0.25, 54000.0, 15);
    assert_slice(&["57000.000", "58000.000", "61000.000"]);
    assert_subscriptions(&ids[4..]);

    fixture.recorder.borrow_mut().clear();

    match (interval, market_data_expiry) {
        (_, true) => {
            let expired_quote = make_quote(
                ids[21][0],
                "101.000",
                "103.000",
                series_id.expiration_ns.as_u64(),
            );
            fixture
                .engine
                .borrow_mut()
                .process_data(Data::Quote(expired_quote));
        }
        (Some(_), false) => {
            let events = fixture
                .clock
                .borrow_mut()
                .advance_time(series_id.expiration_ns, false);
            let handlers = fixture.clock.borrow().match_handlers(events);
            for handler in handlers {
                handler.callback.call(handler.event);
            }

            let definition = make_btc_option("60500.000", OptionKind::Call);
            let pipeline_definition = Box::new(definition.clone());
            let mut engine = fixture.engine.borrow_mut();
            engine.process_pipeline(Data::Instrument(pipeline_definition));
            assert!(engine.has_option_chain_manager(&series_id));
            assert!(fixture.recorder.borrow().is_empty());
            engine.process(&definition);
        }
        (None, false) => {}
    }

    if interval.is_some() || market_data_expiry {
        let recorder = fixture.recorder.borrow();
        assert_eq!(recorder.len(), 42 * 3);
        assert!(
            recorder
                .iter()
                .all(|command| matches!(command, DataCommand::Unsubscribe(_)))
        );
        assert!(!fixture.engine.borrow().has_option_chain_manager(&series_id));
    }

    fixture.dispose();
    assert_subscriptions(&[]);
    assert_eq!(fixture.clock.borrow().timer_count(), 0);
}

#[rstest]
fn test_option_chain_delta_series_expiry_precedes_fixed_retirement(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let fixture = OptionChainFixture::new(clock, &cache);
    let mut ids = vec![make_btc_option("50000.000", OptionKind::Call).id()];

    for index in 51..=61 {
        let option = make_btc_option(&format!("{index}000.000"), OptionKind::Call);
        ids.push(option.id());
        cache.borrow_mut().add_instrument(option).unwrap();
    }

    let atm = Price::from("50000.000");
    fixture.subscribe(0, StrikeRange::Fixed(vec![atm]), None);
    fixture.subscribe(1, StrikeRange::delta(0.25, 0.04).unwrap(), None);
    fixture.resolve_reference_price(atm);
    let mut subscribed = fixture.engine.borrow().subscribed_quotes();
    subscribed.sort_unstable();
    assert_eq!(subscribed, ids[..11]);

    fixture.recorder.borrow_mut().clear();
    let series_id = make_series_id();
    let quote = make_quote(
        ids[0],
        "101.000",
        "103.000",
        series_id.expiration_ns.as_u64(),
    );
    fixture.engine.borrow_mut().process_data(Data::Quote(quote));

    let engine = fixture.engine.borrow();
    assert!(!engine.has_option_chain_manager(&series_id));
    assert!(engine.subscribed_quotes().is_empty());
    drop(engine);
    let commands = fixture.recorder.borrow();
    assert_eq!(commands.len(), 33);
    assert!(
        commands
            .iter()
            .all(|cmd| matches!(cmd, DataCommand::Unsubscribe(_)))
    );
    drop(commands);
    fixture.dispose();
}

#[rstest]
fn test_option_chain_delta_expiry_preserves_direct_quotes_for_pending_candidate(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let fixture = OptionChainFixture::new(clock, &cache);
    let series_id = make_series_id();
    let start = series_id.expiration_ns - DurationNanos::from_secs(4);
    fixture.clock.borrow().set_time(start);

    for strike in 51..=61 {
        let instrument = make_btc_option(&format!("{strike}000.000"), OptionKind::Call);
        cache.borrow_mut().add_instrument(instrument).unwrap();
    }

    fixture.subscribe(0, StrikeRange::delta(0.25, 0.04).unwrap(), Some(1_000));
    fixture.resolve_reference_price(Price::from("50000.000"));

    let instrument_id = make_btc_option("61000.000", OptionKind::Call).id();

    let direct = SubscribeQuotes::new(
        instrument_id,
        Some(fixture.client_id),
        Some(instrument_id.venue),
        UUID4::new(),
        start,
        None,
        None,
    );

    fixture
        .engine
        .borrow_mut()
        .execute_subscribe(SubscribeCommand::Quotes(direct))
        .unwrap();
    fixture.recorder.borrow_mut().clear();

    let atm = make_btc_option("50000.000", OptionKind::Call).id();
    fixture
        .engine
        .borrow_mut()
        .process_data(Data::OptionGreeks(make_option_chain_greeks(atm, 51000.0)));
    advance_clock_and_dispatch(&fixture.clock, 2_000_000_000);
    assert_eq!(
        msgbus::exact_subscriber_count_quotes(switchboard::get_quotes_topic(instrument_id)),
        1
    );
    assert!(fixture.recorder.borrow().is_empty());

    advance_clock_and_dispatch(&fixture.clock, 2_000_000_000);
    let mut engine = fixture.engine.borrow_mut();
    let quote = make_quote(atm, "101.000", "103.000", series_id.expiration_ns.as_u64());
    engine.process_data(Data::Quote(quote));

    assert!(!engine.has_option_chain_manager(&series_id));
    assert_eq!(engine.subscribed_quotes(), [instrument_id]);
    assert!(engine.subscribed_instrument_status().is_empty());
    drop(engine);
    let commands = fixture.recorder.borrow();
    assert_eq!(commands.len(), 33);
    assert!(
        commands
            .iter()
            .all(|command| matches!(command, DataCommand::Unsubscribe(_)))
    );
    drop(commands);
    fixture.dispose();
}

#[rstest]
fn test_option_chain_delta_replaces_candidate_for_same_leg(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let fixture = OptionChainFixture::new(clock, &cache);
    let strike = "50000.000";
    let original = make_btc_option(strike, OptionKind::Call).id();
    let preferred = make_crypto_option(
        "BTC-20240101-50000.000-0.DERIBIT",
        "BTC",
        "BTC",
        strike,
        OptionKind::Call,
        make_series_id().expiration_ns,
    );
    let preferred_id = preferred.id();
    fixture.subscribe(0, StrikeRange::delta(0.25, 0.04).unwrap(), None);
    fixture.resolve_reference_price(Price::from(strike));
    let before = make_quote(original, "101.000", "103.000", 2);
    let mut engine = fixture.engine.borrow_mut();
    engine.process_data(Data::Quote(before));
    let mut greeks = make_option_chain_greeks(original, 50000.0);
    greeks.greeks.delta = 0.25;
    greeks.ts_event = UnixNanos::from(3);
    engine.process_data(Data::OptionGreeks(greeks));

    fixture.recorder.borrow_mut().clear();
    engine.process_pipeline(Data::Instrument(Box::new(preferred.clone())));
    let unrelated = make_btc_option("54000.000", OptionKind::Call).id();
    engine.process_data(Data::Quote(make_quote(unrelated, "1.000", "2.000", 4)));

    assert!(cache.borrow().instrument(&preferred_id).is_some());
    assert_eq!(engine.subscribed_quotes(), [original]);
    assert_eq!(engine.subscribed_instrument_status(), [original]);
    assert!(fixture.recorder.borrow().is_empty());

    for (id, expected) in [(original, 1), (preferred_id, 0)] {
        assert_eq!(
            msgbus::exact_subscriber_count_quotes(switchboard::get_quotes_topic(id)),
            expected
        );
        assert_eq!(
            msgbus::exact_subscriber_count_option_greeks(switchboard::get_option_greeks_topic(id)),
            expected
        );
    }

    greeks.ts_event = UnixNanos::from(5);
    engine.process_data(Data::OptionGreeks(greeks));
    let actor = get_actor_unchecked::<OptionChainActor>(&fixture.actors[0]);
    assert_eq!(actor.slices.len(), 3);
    assert_eq!(actor.slices[2].calls[&Price::from(strike)].quote, before);
    drop(actor);

    engine.process(&preferred);
    assert_eq!(engine.subscribed_quotes(), [preferred_id]);
    let is_subscribe = |cmd: &DataCommand| matches!(cmd, DataCommand::Subscribe(_));
    let handoff: Vec<_> = fixture.recorder.borrow().iter().map(is_subscribe).collect();
    assert_eq!(handoff, [true, true, true, false, false, false]);

    let live = make_quote(preferred_id, "201.000", "203.000", 6);
    engine.process_data(Data::Quote(live));

    fixture.recorder.borrow_mut().clear();
    engine.process_data(Data::InstrumentStatus(InstrumentStatus::new(
        preferred_id,
        MarketStatusAction::Close,
        UnixNanos::from(7),
        UnixNanos::from(7),
        None,
        None,
        Some(false),
        Some(false),
        None,
    )));
    assert_eq!(engine.subscribed_quotes(), [original]);
    let handoff: Vec<_> = fixture.recorder.borrow().iter().map(is_subscribe).collect();
    assert_eq!(handoff, [true, true, true, false, false, false]);

    let resumed = make_quote(original, "301.000", "303.000", 8);
    engine.process_data(Data::Quote(resumed));
    drop(engine);

    let actor = get_actor_unchecked::<OptionChainActor>(&fixture.actors[0]);
    let visible: Vec<_> = actor.slices[3..]
        .iter()
        .map(|slice| &slice.calls[&Price::from(strike)])
        .collect();
    assert_eq!(visible.len(), 2);
    assert_eq!(visible[0].quote, live);
    assert_eq!(visible[0].greeks, None);
    assert_eq!(visible[1].quote, resumed);
    assert_eq!(visible[1].greeks, None);
    drop(actor);
    fixture.dispose();
}

#[rstest]
#[case::okx_black_scholes_first("OKX", None, &[BlackScholes, PriceAdjusted], Some(BlackScholes))]
#[case::okx_price_adjusted_first("OKX", None, &[PriceAdjusted, BlackScholes], Some(BlackScholes))]
#[case::okx_explicit("OKX", Some(json!("price_adjusted")), &[PriceAdjusted, BlackScholes], Some(PriceAdjusted))]
#[case::okx_both("OKX", Some(json!(["PRICE_ADJUSTED", "BLACK_SCHOLES"])), &[PriceAdjusted, BlackScholes], Some(BlackScholes))]
#[case::okx_both_reversed("OKX", Some(json!(["BLACK_SCHOLES", "PRICE_ADJUSTED"])), &[PriceAdjusted, BlackScholes], Some(BlackScholes))]
#[case::deribit_default("DERIBIT", None, &[BlackScholes], Some(BlackScholes))]
#[case::deribit_unavailable("DERIBIT", Some(json!(["PRICE_ADJUSTED"])), &[BlackScholes], None)]
fn test_option_chain_delta_greeks_convention(
    #[case] venue: &str,
    #[case] requested: Option<serde_json::Value>,
    #[case] updates: &[GreeksConvention],
    #[case] expected: Option<GreeksConvention>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let engine = make_option_chain_engine(Rc::clone(&clock), Rc::clone(&cache));
    let client_id = ClientId::from(venue);
    let venue = Venue::from(venue);
    let recorder = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        Rc::clone(&clock),
        Rc::clone(&cache),
        client_id,
        venue,
        Some(venue),
        &recorder,
        &mut engine.borrow_mut(),
    );

    let series_id = OptionSeriesId::new_derived(
        venue,
        "BTC".into(),
        "BTC".into(),
        make_series_id().expiration_ns,
    );
    let strikes = ["50000.000", "51000.000"];

    let ids = strikes.map(|strike| {
        let instrument = make_crypto_option(
            &format!("BTC-20240101-{strike}-C.{venue}"),
            "BTC",
            "BTC",
            strike,
            OptionKind::Call,
            series_id.expiration_ns,
        );
        let id = instrument.id();
        cache.borrow_mut().add_instrument(instrument).unwrap();
        id
    });

    let feed_params = requested
        .as_ref()
        .map(|_| Params::new([("route".to_string(), json!(37))].into()));

    let params = requested.map(|value| {
        Params::new(
            [
                ("greeks_convention".to_string(), value),
                ("route".to_string(), json!(37)),
            ]
            .into(),
        )
    });

    let owner_id = UUID4::new();
    let (handler, saver) = get_typed_message_saving_handler::<OptionChainSlice>(None);
    msgbus::register_option_chain_owner(owner_id, handler);
    let command = SubscribeOptionChain::new(
        series_id,
        StrikeRange::delta(0.25, 0.03).unwrap(),
        Some(1_000),
        owner_id,
        UnixNanos::default(),
        Some(client_id),
        Some(venue),
        params,
    )
    .unwrap();
    engine
        .borrow_mut()
        .execute_subscribe(SubscribeCommand::OptionChain(command))
        .unwrap();
    let request_id = option_chain_reference_price_request_id(&recorder);
    engine
        .borrow_mut()
        .response(DataResponse::OptionChainReferencePrice(
            OptionChainReferencePriceResponse::new(
                request_id,
                client_id,
                series_id,
                Some(Price::from(strikes[0])),
                UnixNanos::from(1),
                None,
            ),
        ));

    let make_greeks = |index: usize, convention| {
        let (delta, forward, ts_event) = match (index, convention) {
            (0, BlackScholes) => (0.25, 50000.0, 11),
            (0, PriceAdjusted) => (0.45, 51000.0, 12),
            (_, BlackScholes) => (0.15, 50000.0, 11),
            (_, PriceAdjusted) => (0.25, 51000.0, 12),
        };

        let mut greeks = make_option_chain_greeks(ids[index], forward);
        greeks.convention = convention;
        greeks.greeks.delta = delta;
        greeks.ts_event = UnixNanos::from(ts_event);
        greeks
    };

    let [first_quote, second_quote] = ids.map(|id| make_quote(id, "101.000", "103.000", 5));
    engine.borrow_mut().process_data(Data::Quote(first_quote));
    for &convention in updates {
        let mut engine = engine.borrow_mut();
        engine.process_data(Data::OptionGreeks(make_greeks(0, convention)));
        engine.process_data(Data::OptionGreeks(make_greeks(1, convention)));
    }

    engine.borrow_mut().process_data(Data::Quote(second_quote));
    advance_clock_and_dispatch(&clock, 2_000_000_000);

    let mut child_subscriptions: Vec<_> = recorder
        .borrow()
        .iter()
        .filter_map(|command| match command {
            DataCommand::Subscribe(SubscribeCommand::OptionGreeks(command)) => Some((
                command.single_instrument_id().unwrap(),
                command.client_id,
                command.params.clone(),
            )),
            _ => None,
        })
        .collect();

    child_subscriptions.sort_unstable_by_key(|(id, _, _)| *id);
    assert_eq!(
        child_subscriptions,
        ids.map(|id| (id, Some(client_id), feed_params.clone())),
    );

    let messages = saver.get_messages();
    assert_eq!(messages.len(), 1);
    let slice = &messages[0];
    assert!(slice.puts.is_empty());

    if let Some(convention) = expected {
        let index = usize::from(convention == PriceAdjusted);
        let strike = Price::from(strikes[index]);
        assert_eq!(slice.calls.keys().copied().collect::<Vec<_>>(), [strike]);
        let greeks = make_greeks(index, convention);
        assert_eq!(slice.get_call_greeks(&strike), Some(&greeks));
        assert_eq!(slice.atm_strike, Some(strike));
        assert_eq!(slice.ts_event, greeks.ts_event);
    } else {
        assert_eq!(
            slice.calls.keys().copied().collect::<Vec<_>>(),
            strikes.map(Price::from)
        );
        assert!(slice.calls.values().all(|leg| leg.greeks.is_none()));
        assert_eq!(slice.atm_strike, Some(Price::from(strikes[0])));
        assert_eq!(slice.ts_event, UnixNanos::from(5));
    }

    msgbus::unregister_option_chain_owner(owner_id);
}

#[rstest]
fn test_option_chain_without_sample_bootstraps_from_live_data(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let _ = msgbus::get_message_bus();
    let data_engine = make_option_chain_engine(Rc::clone(&clock), Rc::clone(&cache));
    let client_id = ClientId::new("DERIBIT");
    let venue = Venue::new("DERIBIT");
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));
    register_mock_client(
        Rc::clone(&clock),
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
    let data_engine = make_option_chain_engine(Rc::clone(&clock), Rc::clone(&cache));
    let client_id = ClientId::new("DERIBIT");
    let venue = Venue::new("DERIBIT");
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));
    register_mock_client(
        Rc::clone(&clock),
        Rc::clone(&cache),
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

    let other_series_id = OptionSeriesId::new_derived(
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
    let data_engine = make_option_chain_engine(Rc::clone(&clock), Rc::clone(&cache));
    let client_id = ClientId::new("DERIBIT");
    let venue = Venue::new("DERIBIT");
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));
    register_mock_client(
        Rc::clone(&clock),
        Rc::clone(&cache),
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
    let data_engine = make_option_chain_engine(clock, Rc::clone(&cache));
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
    let data_engine = make_option_chain_engine(Rc::clone(&clock), Rc::clone(&cache));
    let client_id = ClientId::new("DERIBIT");
    let venue = Venue::new("DERIBIT");
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));
    register_mock_client(
        Rc::clone(&clock),
        Rc::clone(&cache),
        client_id,
        venue,
        Some(venue),
        &recorder,
        &mut data_engine.borrow_mut(),
    );

    let first_series = make_series_id();
    let second_expiration = first_series.expiration_ns + DurationNanos::from_secs(5);

    let second_series = OptionSeriesId::new_derived(
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
    let data_engine = make_option_chain_engine(Rc::clone(&clock), Rc::clone(&cache));
    let client_id = ClientId::new("DERIBIT");
    let venue = Venue::new("DERIBIT");
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));
    register_mock_client(
        clock,
        Rc::clone(&cache),
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

    let params = Params::new(
        [
            ("greeks_convention".to_string(), json!("PRICE_ADJUSTED")),
            ("route".to_string(), json!(37)),
        ]
        .into(),
    );

    let feed_params = Params::new([("route".to_string(), json!(37))].into());
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
                Some(params.clone()),
            )
            .unwrap(),
        )));

    let request_id = {
        let recorded = recorder.borrow();

        let [DataCommand::Request(RequestCommand::OptionChainReferencePrice(request))] =
            recorded.as_slice()
        else {
            panic!("Expected one option-chain reference price request");
        };

        assert_eq!(request.params.as_ref(), Some(&params));
        request.request_id
    };

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
                if cmd.single_instrument_id().unwrap() == sample_id && cmd.client_id == Some(client_id)
                    && cmd.params.as_ref() == Some(&feed_params)
        )
    }));
    recorder.borrow_mut().clear();

    let greeks = make_option_chain_greeks(sample_id, 50000.0);
    data_engine.borrow_mut().process(&greeks);
    assert!(recorder.borrow().is_empty());

    data_engine
        .borrow_mut()
        .process_data(Data::OptionGreeks(OptionGreeks {
            convention: PriceAdjusted,
            ..greeks
        }));

    let recorded = recorder.borrow();
    let quote_subscriptions = recorded
        .iter()
        .filter(|cmd| matches!(cmd, DataCommand::Subscribe(SubscribeCommand::Quotes(_))))
        .count();

    let greeks_subscriptions: Vec<_> = recorded
        .iter()
        .filter_map(|cmd| match cmd {
            DataCommand::Subscribe(SubscribeCommand::OptionGreeks(cmd)) => Some(cmd),
            _ => None,
        })
        .collect();

    let greeks_unsubscriptions: Vec<InstrumentId> = recorded
        .iter()
        .filter_map(|cmd| match cmd {
            DataCommand::Unsubscribe(UnsubscribeCommand::OptionGreeks(cmd)) => {
                Some(cmd.single_instrument_id().unwrap())
            }
            _ => None,
        })
        .collect();

    assert_eq!(quote_subscriptions, 2);
    assert_eq!(greeks_subscriptions.len(), 2);
    assert!(
        greeks_subscriptions
            .iter()
            .all(|cmd| cmd.params.as_ref() == Some(&feed_params))
    );
    assert_eq!(greeks_unsubscriptions, vec![sample_id]);
}

#[rstest]
fn test_option_chain_greeks_bootstrap_holds_subscription_ownership(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let _ = msgbus::get_message_bus();
    let data_engine = make_option_chain_engine(Rc::clone(&clock), Rc::clone(&cache));
    let client_id = ClientId::new("DERIBIT");
    let venue = Venue::new("DERIBIT");
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));
    register_mock_client(
        clock,
        Rc::clone(&cache),
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
    let data_engine = make_option_chain_engine(Rc::clone(&clock), Rc::clone(&cache));
    let client_id = ClientId::new("DERIBIT");
    let venue = Venue::new("DERIBIT");
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));
    register_mock_client(
        clock,
        Rc::clone(&cache),
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
    #[values(false, true)] stopped: bool,
    #[case] dispatch: OptionGreeksDispatch,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let _ = msgbus::get_message_bus();
    let data_engine = make_option_chain_engine(Rc::clone(&clock), Rc::clone(&cache));

    let client_id = ClientId::new("DERIBIT");
    let venue = Venue::new("DERIBIT");
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));

    register_mock_client(
        clock,
        Rc::clone(&cache),
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
    let owner_id = UUID4::new();
    let (handler, saver) = get_typed_message_saving_handler::<OptionChainSlice>(None);
    msgbus::register_option_chain_owner(owner_id, handler);

    let cmd = DataCommand::Subscribe(SubscribeCommand::OptionChain(
        SubscribeOptionChain::new(
            series_id,
            StrikeRange::AtmRelative {
                strikes_above: 1,
                strikes_below: 1,
            },
            None,
            owner_id,
            UnixNanos::default(),
            Some(client_id),
            Some(venue),
            None,
        )
        .unwrap(),
    ));

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
        bootstrap_subscriptions[0].single_instrument_id().unwrap(),
        bootstrap_instrument_id
    );
    assert_eq!(bootstrap_subscriptions[0].client_id, Some(client_id));

    recorder.borrow_mut().clear();

    if stopped {
        data_engine.borrow_mut().stop();
    }

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

    assert_eq!(quote_subs, if stopped { 0 } else { 6 });
    assert_eq!(greeks_subs, if stopped { 0 } else { 5 });
    assert_eq!(status_subs, if stopped { 0 } else { 6 });
    drop(recorded);

    assert!(saver.get_messages().is_empty());

    let quote = QuoteTick::new(
        call_id,
        Price::from("100.00"),
        Price::from("101.00"),
        Quantity::from("1.0"),
        Quantity::from("1.0"),
        UnixNanos::from(2_u64),
        UnixNanos::from(2_u64),
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
    let data_engine = make_option_chain_engine(Rc::clone(&clock), Rc::clone(&cache));
    let client_id = ClientId::new("DERIBIT");
    let venue = Venue::new("DERIBIT");
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));
    register_mock_client(
        clock,
        Rc::clone(&cache),
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
    let owner_id = UUID4::new();
    let (handler, _saver) = get_typed_message_saving_handler::<OptionChainSlice>(None);
    msgbus::register_option_chain_owner(owner_id, handler);
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
                owner_id,
                UnixNanos::default(),
                Some(client_id),
                Some(venue),
                None,
            )
            .unwrap(),
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
                    Some(cmd.single_instrument_id().unwrap())
                }
                _ => None,
            })
            .collect()
    };

    assert!(greeks_unsubscribes(&recorder).is_empty());

    msgbus::unregister_option_chain_owner(owner_id);
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
    let data_engine = make_option_chain_engine(Rc::clone(&clock), Rc::clone(&cache));

    let client_id = ClientId::new("DERIBIT");
    let venue = Venue::new("DERIBIT");
    let recorder = Rc::new(RefCell::new(Vec::<DataCommand>::new()));

    register_mock_client(
        clock,
        Rc::clone(&cache),
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

#[rstest]
fn test_external_option_chain_anonymous_release_checks_all_clients(
    _stub_msgbus: Rc<RefCell<MessageBus>>,
) {
    let clients = [ClientId::from("EXTERNAL-A"), ClientId::from("EXTERNAL-B")];

    let config = DataEngineConfig {
        external_clients: Some(clients.to_vec()),
        ..Default::default()
    };

    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let mut engine = DataEngine::new(clock, Rc::new(RefCell::new(Cache::default())), Some(config));
    let series_id = make_series_id();
    let mut commands = Vec::new();

    let savers = clients.map(|client_id| {
        let topic = format!("commands.data.{client_id}");
        let (handler, saver) = get_any_saving_handler::<UnsubscribeCommand>(None);
        msgbus::subscribe_any(topic.as_str().into(), handler, None);
        let command = make_subscribe_option_chain(
            series_id,
            vec![Price::from("50000")],
            Some(client_id),
            Some(series_id.venue),
        );
        engine.execute(command.clone());
        commands.push(command);
        saver
    });

    engine.execute(make_unsubscribe_option_chain(
        series_id,
        Some(clients[0]),
        Some(series_id.venue),
    ));
    assert!(savers.iter().all(|saver| saver.get_messages().is_empty()));
    engine.execute(option_chain_unsubscribe(&commands[0]));
    assert_eq!(savers[0].get_messages().len(), 1);
    assert!(savers[1].get_messages().is_empty());
    engine.execute(make_unsubscribe_option_chain(
        series_id,
        Some(clients[1]),
        Some(series_id.venue),
    ));
    assert_eq!(savers[1].get_messages().len(), 1);
}

#[rstest]
#[case::terminal_price(false, Some(Price::from("50000.000")))]
#[case::terminal_fallback(false, None)]
#[case::partial_price(true, Some(Price::from("50000.000")))]
#[case::partial_fallback(true, None)]
fn test_option_chain_status_retirement_does_not_revive_strikes(
    #[case] partial: bool,
    #[case] price: Option<Price>,
    #[values(false, true)] replay: bool,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let fixture = OptionChainFixture::new(clock, &cache);
    let open = make_btc_option("51000.000", OptionKind::Call);
    let open_id = open.id();
    if partial {
        fixture.engine.borrow_mut().process(&open);
    }

    fixture.subscribe(
        0,
        StrikeRange::Fixed(vec![Price::from("50000.000"), Price::from("51000.000")]),
        Some(1_000),
    );
    fixture.subscribe(
        1,
        StrikeRange::AtmRelative {
            strikes_above: 1,
            strikes_below: 1,
        },
        Some(1_000),
    );

    let request_id = option_chain_reference_price_request_id(&fixture.recorder);
    let closed = make_btc_option("50000.000", OptionKind::Call);
    fixture
        .engine
        .borrow_mut()
        .process_data(Data::InstrumentStatus(InstrumentStatus::new(
            closed.id(),
            MarketStatusAction::Close,
            UnixNanos::from(3),
            UnixNanos::from(5),
            None,
            None,
            Some(false),
            Some(false),
            None,
        )));
    fixture.recorder.borrow_mut().clear();
    if replay {
        fixture
            .engine
            .borrow_mut()
            .process_pipeline(Data::InstrumentStatus(InstrumentStatus::new(
                closed.id(),
                MarketStatusAction::Trading,
                UnixNanos::from(1),
                UnixNanos::from(9),
                None,
                None,
                Some(true),
                Some(true),
                None,
            )));
        assert_eq!(
            cache
                .borrow()
                .instrument_status(&closed.id())
                .unwrap()
                .action,
            MarketStatusAction::Trading
        );
    }

    fixture.engine.borrow_mut().process(&closed);
    fixture
        .engine
        .borrow_mut()
        .response(DataResponse::OptionChainReferencePrice(
            OptionChainReferencePriceResponse::new(
                request_id,
                fixture.client_id,
                make_series_id(),
                price,
                UnixNanos::from(7),
                None,
            ),
        ));

    if partial {
        fixture
            .engine
            .borrow_mut()
            .process_data(Data::OptionGreeks(make_option_chain_greeks(
                open_id, 51000.0,
            )));
        let quote = make_quote(open_id, "101.000", "103.000", 11);
        fixture.engine.borrow_mut().process_data(Data::Quote(quote));
        advance_clock_and_dispatch(&fixture.clock, 2_000_000_000);
        for id in &fixture.actors {
            let actor = get_actor_unchecked::<OptionChainActor>(id);
            let slice = actor
                .slices
                .last()
                .expect("remaining strike reaches each owner");
            assert_eq!(
                slice.calls.keys().copied().collect::<Vec<_>>(),
                [Price::from("51000.000")]
            );
            assert_eq!(slice.calls[&Price::from("51000.000")].quote, quote);
        }
    }

    assert_eq!(
        fixture.engine.borrow().pending_option_chain_request_count(),
        0
    );
    assert_eq!(
        fixture
            .engine
            .borrow()
            .has_option_chain_manager(&make_series_id()),
        partial
    );
    assert_eq!(
        fixture.clock.borrow().timer_count(),
        if partial { 2 } else { 0 }
    );
    assert_eq!(
        fixture.engine.borrow().subscribed_quotes(),
        if partial { vec![open_id] } else { vec![] }
    );
    assert!(fixture.recorder.borrow().is_empty());
    fixture.dispose();
}

#[rstest]
#[case::missing(false, false)]
#[case::replaced(true, false)]
#[case::default_replaced(true, true)]
fn test_option_chain_client_removal_never_releases_replacement_feeds(
    #[case] acquired: bool,
    #[case] default_replacement: bool,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let fixture = OptionChainFixture::new(Rc::clone(&clock), &cache);
    let range = StrikeRange::Fixed(vec![Price::from("50000.000")]);
    if acquired {
        fixture.subscribe(0, range.clone(), None);
    }

    if default_replacement {
        let client = MockDataClient::new_with_recorder(
            clock,
            cache,
            fixture.client_id,
            Some(Venue::from("DERIBIT")),
            Some(Rc::clone(&fixture.recorder)),
        );
        fixture
            .engine
            .borrow_mut()
            .register_default_client(DataClientAdapter::new(
                fixture.client_id,
                None,
                true,
                true,
                Box::new(client),
            ));
    } else {
        fixture
            .engine
            .borrow_mut()
            .deregister_client(&fixture.client_id);
        if !acquired {
            fixture.subscribe(0, range.clone(), None);
        }

        register_mock_client(
            clock,
            cache,
            fixture.client_id,
            Venue::from("DERIBIT"),
            Some(Venue::from("DERIBIT")),
            &fixture.recorder,
            &mut fixture.engine.borrow_mut(),
        );
    }

    fixture.subscribe(1, range, None);
    fixture.recorder.borrow_mut().clear();
    get_actor_unchecked::<OptionChainActor>(&fixture.actors[0])
        .unsubscribe_option_chain(make_series_id(), None);
    let instrument_id = make_btc_option("50000.000", OptionKind::Call).id();
    let quote = make_quote(instrument_id, "101.000", "103.000", 11);
    fixture.engine.borrow_mut().process_data(Data::Quote(quote));
    let actor = get_actor_unchecked::<OptionChainActor>(&fixture.actors[1]);
    let slice = actor.slices.last().expect("peer still receives its slice");
    assert_eq!(slice.calls[&Price::from("50000.000")].quote, quote);
    drop(actor);
    assert!(fixture.recorder.borrow().is_empty());
    assert_eq!(fixture.engine.borrow().subscribed_quotes(), [instrument_id]);
    assert_eq!(
        fixture.engine.borrow().get_clients()[0]
            .subscriptions_option_greeks
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        [instrument_id]
    );
    assert_eq!(
        fixture.engine.borrow().subscribed_instrument_status(),
        [instrument_id]
    );
    fixture.dispose();
    assert_eq!(fixture.recorder.borrow().len(), 3);
}

#[rstest]
#[case::ordinary(false, false)]
#[case::failed_explicit(true, false)]
#[case::failed_routed(true, true)]
fn test_option_chain_client_removal_cancels_pending_owner_only(
    #[case] failed: bool,
    #[case] inferred: bool,
    #[values(false, true)] pending_peer: bool,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let fixture = OptionChainFixture::new(Rc::clone(&clock), &cache);
    let other = ClientId::from("OTHER-CHAIN");
    let recorder = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        Rc::clone(&clock),
        Rc::clone(&cache),
        other,
        Venue::from("DERIBIT"),
        None,
        &recorder,
        &mut fixture.engine.borrow_mut(),
    );
    get_actor_unchecked::<OptionChainActor>(&fixture.actors[0])
        .subscribe_option_chain(
            make_series_id(),
            StrikeRange::AtmRelative {
                strikes_above: 1,
                strikes_below: 1,
            },
            Some(1_000),
            if inferred {
                None
            } else {
                Some(fixture.client_id)
            },
            Some(fixture.params(0)),
        )
        .unwrap();

    let request_id = option_chain_reference_price_request_id(&fixture.recorder);
    get_actor_unchecked::<OptionChainActor>(&fixture.actors[1])
        .subscribe_option_chain(
            make_series_id(),
            if pending_peer {
                StrikeRange::AtmRelative {
                    strikes_above: 1,
                    strikes_below: 1,
                }
            } else {
                StrikeRange::Fixed(vec![Price::from("50000.000")])
            },
            None,
            Some(other),
            Some(fixture.params(1)),
        )
        .unwrap();

    let other_request = pending_peer.then(|| option_chain_reference_price_request_id(&recorder));

    if failed {
        fixture.engine.borrow_mut().process(&DataRequestFailed {
            request_id,
            reason: "Reference price unavailable".to_string(),
            response: None,
        });
    }

    recorder.borrow_mut().clear();
    fixture
        .engine
        .borrow_mut()
        .deregister_client(&fixture.client_id);
    assert_eq!(
        fixture.engine.borrow().pending_option_chain_request_count(),
        usize::from(pending_peer)
    );
    assert_eq!(
        fixture.clock.borrow().timer_count(),
        usize::from(pending_peer)
    );
    register_mock_client(
        clock,
        cache,
        fixture.client_id,
        Venue::from("DERIBIT"),
        Some(Venue::from("DERIBIT")),
        &fixture.recorder,
        &mut fixture.engine.borrow_mut(),
    );
    fixture
        .engine
        .borrow_mut()
        .response(DataResponse::OptionChainReferencePrice(
            OptionChainReferencePriceResponse::new(
                request_id,
                fixture.client_id,
                make_series_id(),
                Some(Price::from("50000.000")),
                UnixNanos::from(1),
                None,
            ),
        ));

    if let Some(request_id) = other_request {
        fixture
            .engine
            .borrow_mut()
            .response(DataResponse::OptionChainReferencePrice(
                OptionChainReferencePriceResponse::new(
                    request_id,
                    other,
                    make_series_id(),
                    Some(Price::from("50000.000")),
                    UnixNanos::from(3),
                    None,
                ),
            ));
        recorder.borrow_mut().clear();
    }

    get_actor_unchecked::<OptionChainActor>(&fixture.actors[0])
        .unsubscribe_option_chain(make_series_id(), None);
    let instrument_id = make_btc_option("50000.000", OptionKind::Call).id();
    let quote = make_quote(instrument_id, "101.000", "103.000", 11);
    fixture.engine.borrow_mut().process_data(Data::Quote(quote));
    let actor = get_actor_unchecked::<OptionChainActor>(&fixture.actors[1]);
    assert_eq!(
        actor.slices.last().unwrap().calls[&Price::from("50000.000")].quote,
        quote
    );
    drop(actor);
    assert_eq!(
        fixture.engine.borrow().pending_option_chain_request_count(),
        0
    );
    assert_eq!(fixture.clock.borrow().timer_count(), 0);
    assert!(recorder.borrow().is_empty());
    fixture.dispose();
}

#[rstest]
fn test_option_chain_close_before_definition_stays_retired(
    #[values(false, true)] active_series: bool,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let fixture = OptionChainFixture::new(clock, &cache);
    if active_series {
        fixture.subscribe(
            0,
            StrikeRange::Fixed(vec![Price::from("50000.000"), Price::from("51000.000")]),
            None,
        );
    }

    fixture.recorder.borrow_mut().clear();
    let closed = make_btc_option("51000.000", OptionKind::Call);
    fixture
        .engine
        .borrow_mut()
        .process_data(Data::InstrumentStatus(InstrumentStatus::new(
            closed.id(),
            MarketStatusAction::Close,
            UnixNanos::from(5),
            UnixNanos::from(7),
            None,
            None,
            Some(false),
            Some(false),
            None,
        )));
    fixture
        .engine
        .borrow_mut()
        .process_pipeline(Data::InstrumentStatus(InstrumentStatus::new(
            closed.id(),
            MarketStatusAction::Trading,
            UnixNanos::from(1),
            UnixNanos::from(9),
            None,
            None,
            Some(true),
            Some(true),
            None,
        )));
    fixture.engine.borrow_mut().process(&closed);
    fixture.subscribe(1, StrikeRange::Fixed(vec![Price::from("51000.000")]), None);
    fixture
        .engine
        .borrow_mut()
        .process_data(Data::Quote(make_quote(
            closed.id(),
            "201.000",
            "203.000",
            11,
        )));
    let known = make_btc_option("50000.000", OptionKind::Call).id();

    if active_series {
        fixture
            .engine
            .borrow_mut()
            .process_data(Data::Quote(make_quote(known, "101.000", "103.000", 13)));
        let actor = get_actor_unchecked::<OptionChainActor>(&fixture.actors[0]);
        assert_eq!(
            actor
                .slices
                .last()
                .unwrap()
                .calls
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            [Price::from("50000.000")]
        );
    }

    assert!(fixture.recorder.borrow().is_empty());
    assert_eq!(
        fixture.engine.borrow().subscribed_quotes(),
        if active_series { vec![known] } else { vec![] }
    );
    assert!(
        get_actor_unchecked::<OptionChainActor>(&fixture.actors[1])
            .slices
            .is_empty()
    );
    fixture.dispose();
}

#[rstest]
fn test_option_chain_replayed_close_does_not_retire_new_live_owner(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let fixture = OptionChainFixture::new(clock, &cache);
    let range = StrikeRange::Fixed(vec![Price::from("50000.000")]);
    fixture.subscribe(0, range.clone(), None);
    let instrument_id = make_btc_option("50000.000", OptionKind::Call).id();
    fixture
        .engine
        .borrow_mut()
        .process_pipeline(Data::InstrumentStatus(InstrumentStatus::new(
            instrument_id,
            MarketStatusAction::Close,
            UnixNanos::from(1),
            UnixNanos::from(9),
            None,
            None,
            Some(false),
            Some(false),
            None,
        )));
    fixture.subscribe(1, range, None);
    let quote = make_quote(instrument_id, "101.000", "103.000", 11);
    fixture.engine.borrow_mut().process_data(Data::Quote(quote));
    for id in &fixture.actors {
        let actor = get_actor_unchecked::<OptionChainActor>(id);
        assert_eq!(
            actor.slices.last().unwrap().calls[&Price::from("50000.000")].quote,
            quote
        );
    }

    fixture.dispose();
}

#[rstest]
#[case::disconnect(false)]
#[case::replacement(true)]
fn test_option_chain_retirement_tracks_live_lifecycle(
    #[values(false, true)] trading: bool,
    #[case] replace: bool,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let fixture = OptionChainFixture::new(Rc::clone(&clock), &cache);
    let instrument_id = make_btc_option("50000.000", OptionKind::Call).id();
    fixture
        .engine
        .borrow_mut()
        .process_data(Data::InstrumentStatus(InstrumentStatus::new(
            instrument_id,
            MarketStatusAction::Close,
            UnixNanos::from(5),
            UnixNanos::from(7),
            None,
            None,
            Some(false),
            Some(false),
            None,
        )));

    if replace {
        fixture
            .engine
            .borrow_mut()
            .deregister_client(&fixture.client_id);
        register_mock_client(
            clock,
            cache,
            fixture.client_id,
            Venue::from("DERIBIT"),
            Some(Venue::from("DERIBIT")),
            &fixture.recorder,
            &mut fixture.engine.borrow_mut(),
        );
    } else {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        runtime
            .block_on(fixture.engine.borrow_mut().disconnect())
            .unwrap();
        runtime
            .block_on(fixture.engine.borrow_mut().connect())
            .unwrap();
    }

    fixture
        .engine
        .borrow_mut()
        .process_pipeline(Data::InstrumentStatus(InstrumentStatus::new(
            instrument_id,
            MarketStatusAction::Trading,
            UnixNanos::from(1),
            UnixNanos::from(9),
            None,
            None,
            Some(true),
            Some(true),
            None,
        )));
    let range = StrikeRange::Fixed(vec![Price::from("50000.000")]);
    fixture.subscribe(0, range.clone(), None);
    assert!(fixture.recorder.borrow().is_empty());
    assert!(fixture.engine.borrow().subscribed_quotes().is_empty());

    if trading {
        fixture
            .engine
            .borrow_mut()
            .process_data(Data::InstrumentStatus(InstrumentStatus::new(
                instrument_id,
                MarketStatusAction::Trading,
                UnixNanos::from(11),
                UnixNanos::from(13),
                None,
                None,
                Some(true),
                Some(true),
                None,
            )));
    } else {
        fixture.engine.borrow_mut().reset();
    }

    get_actor_unchecked::<OptionChainActor>(&fixture.actors[0])
        .unsubscribe_option_chain(make_series_id(), None);
    fixture.subscribe(0, range, None);
    let quote = make_quote(instrument_id, "101.000", "103.000", 11);
    fixture.engine.borrow_mut().process_data(Data::Quote(quote));
    let actor = get_actor_unchecked::<OptionChainActor>(&fixture.actors[0]);
    assert_eq!(
        actor.slices.last().unwrap().calls[&Price::from("50000.000")].quote,
        quote
    );
    drop(actor);
    assert_eq!(fixture.engine.borrow().subscribed_quotes(), [instrument_id]);
    fixture.dispose();
}

#[rstest]
fn test_option_chain_backtest_keeps_reference_price_request_client(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let fixture = OptionChainFixture::new(Rc::clone(&clock), &cache);
    let backtest = ClientId::from("BACKTEST");
    let recorder = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        backtest,
        Venue::from("DERIBIT"),
        None,
        &recorder,
        &mut fixture.engine.borrow_mut(),
    );
    fixture.subscribe(
        0,
        StrikeRange::AtmRelative {
            strikes_above: 1,
            strikes_below: 1,
        },
        None,
    );

    let request_id = option_chain_reference_price_request_id(&fixture.recorder);
    assert!(recorder.borrow().is_empty());
    fixture
        .engine
        .borrow_mut()
        .response(DataResponse::OptionChainReferencePrice(
            OptionChainReferencePriceResponse::new(
                request_id,
                fixture.client_id,
                make_series_id(),
                Some(Price::from("50000.000")),
                UnixNanos::from(1),
                None,
            ),
        ));

    let commands = recorder.borrow().clone();
    assert_eq!(commands.len(), 3);

    for command in commands {
        let DataCommand::Subscribe(command) = command else {
            panic!("expected child subscribe");
        };

        assert_eq!(command.client_id(), Some(&backtest));
    }

    assert_eq!(fixture.recorder.borrow().len(), 1);
    fixture.dispose();
}

#[rstest]
fn test_option_chain_resubscribe_reuses_timer_slots(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let fixture = OptionChainFixture::new(clock, &cache);
    let range = StrikeRange::Fixed(vec![Price::from("50000.000")]);
    fixture.subscribe(0, range.clone(), Some(1_000));
    fixture.subscribe(1, range.clone(), Some(2_000));
    let mut initial = fixture
        .clock
        .borrow()
        .timer_names()
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    initial.sort_unstable();
    assert_eq!(initial.len(), 2);
    get_actor_unchecked::<OptionChainActor>(&fixture.actors[0])
        .unsubscribe_option_chain(make_series_id(), None);
    fixture.subscribe(0, range, Some(3_000));
    let mut current = fixture
        .clock
        .borrow()
        .timer_names()
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    current.sort_unstable();

    assert_eq!(current, initial);
    let instrument_id = make_btc_option("50000.000", OptionKind::Call).id();
    let quote = make_quote(instrument_id, "101.000", "103.000", 1);
    fixture.engine.borrow_mut().process_data(Data::Quote(quote));
    advance_clock_and_dispatch(&fixture.clock, 6_000_000_000);
    for id in &fixture.actors {
        let actor = get_actor_unchecked::<OptionChainActor>(id);
        let slice = actor
            .slices
            .last()
            .unwrap_or_else(|| panic!("owner {id} has no slice"));
        assert_eq!(slice.calls[&Price::from("50000.000")].quote, quote);
    }

    fixture.dispose();
    assert_eq!(fixture.clock.borrow().timer_count(), 0);
}

fn make_btc_option(strike: &str, kind: OptionKind) -> InstrumentAny {
    let kind_char = match kind {
        OptionKind::Call => "C",
        OptionKind::Put => "P",
    };

    let symbol = format!("BTC-20240101-{strike}-{kind_char}.DERIBIT");
    let expiration_ns = UnixNanos::from(1_704_067_200_000_000_000_u64);
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
        ts_event: UnixNanos::from(1_u64),
        ts_init: UnixNanos::from(1_u64),
    }
}

fn make_series_id() -> OptionSeriesId {
    OptionSeriesId::new_derived(
        Venue::new("DERIBIT"),
        ustr::Ustr::from("BTC"),
        ustr::Ustr::from("BTC"),
        UnixNanos::from(1_704_067_200_000_000_000_u64),
    )
}

#[rstest]
fn test_option_chain_raw_slice_timestamps(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let fixture = OptionChainFixture::new(clock, &cache);
    fixture.subscribe(0, StrikeRange::Fixed(vec![Price::from("50000.000")]), None);
    let instrument_id = make_btc_option("50000.000", OptionKind::Call).id();

    let greeks = OptionGreeks {
        underlying_price: None,
        ts_event: UnixNanos::from(20),
        ts_init: UnixNanos::from(25),
        ..make_option_chain_greeks(instrument_id, 50000.0)
    };

    let older_greeks = OptionGreeks {
        ts_event: UnixNanos::from(15),
        ts_init: UnixNanos::from(35),
        ..greeks
    };

    let updates = [
        (
            100,
            Data::Quote(make_quote(instrument_id, "101", "103", 10)),
        ),
        (200, Data::OptionGreeks(greeks)),
        (300, Data::OptionGreeks(older_greeks)),
        (400, Data::Quote(make_quote(instrument_id, "102", "104", 5))),
    ];

    for (publication_time, update) in updates {
        fixture.clock.borrow().set_time(publication_time.into());
        fixture.engine.borrow_mut().process_data(update);
    }

    let actor = get_actor_unchecked::<OptionChainActor>(&fixture.actors[0]);
    let timestamps: Vec<_> = actor
        .slices
        .iter()
        .map(|slice| (slice.ts_event.as_u64(), slice.ts_init.as_u64()))
        .collect();
    assert_eq!(timestamps, [(10, 100), (20, 200), (20, 300), (20, 400)]);
    drop(actor);
    fixture.dispose();
}

#[rstest]
fn test_option_chain_greeks_expiry_applies_across_conventions(
    #[values(false, true)] overlapping_owner: bool,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let fixture = OptionChainFixture::new(clock, &cache);
    let series = make_series_id();
    let strike = Price::from("50000.000");
    fixture.subscribe(0, StrikeRange::Fixed(vec![strike]), None);

    if overlapping_owner {
        let params =
            Params::new([("greeks_convention".to_string(), json!("PRICE_ADJUSTED"))].into());
        get_actor_unchecked::<OptionChainActor>(&fixture.actors[1])
            .subscribe_option_chain(
                series,
                StrikeRange::Fixed(vec![strike]),
                None,
                Some(fixture.client_id),
                Some(params),
            )
            .unwrap();
    }

    let instrument = make_btc_option("50000.000", OptionKind::Call);
    let instrument_id = instrument.id();
    let quote = make_quote(instrument_id, "101", "103", 10);

    let accepted = OptionGreeks {
        ts_event: series.expiration_ns - DurationNanos::new(1),
        ..make_option_chain_greeks(instrument_id, 50000.0)
    };

    {
        let mut engine = fixture.engine.borrow_mut();
        engine.process_data(Data::Quote(quote));
        engine.process_data(Data::OptionGreeks(OptionGreeks {
            convention: PriceAdjusted,
            ts_event: series.expiration_ns,
            ..accepted
        }));

        engine.process(&instrument);
        engine.process_data(Data::OptionGreeks(accepted));
        let update = make_quote(instrument_id, "102", "104", 11);
        engine.process_data(Data::Quote(update));

        assert!(!engine.has_option_chain_manager(&series));
        assert!(engine.subscribed_quotes().is_empty());
        assert!(
            engine.get_clients()[0]
                .subscriptions_option_greeks
                .is_empty()
        );
    }

    for &id in &fixture.actors[..if overlapping_owner { 2 } else { 1 }] {
        let actor = get_actor_unchecked::<OptionChainActor>(&id);
        assert_eq!(actor.slices.len(), 1);
        assert_eq!(actor.slices[0].calls[&strike].quote, quote);
        assert_eq!(actor.slices[0].get_call_greeks(&strike), None);
    }

    fixture.dispose();
}

#[rstest]
fn test_option_chain_timer_slice_timestamps(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let fixture = OptionChainFixture::new(clock, &cache);
    fixture.subscribe(
        0,
        StrikeRange::Fixed(vec![Price::from("50000.000")]),
        Some(1_000),
    );
    let instrument_id = make_btc_option("50000.000", OptionKind::Call).id();
    advance_clock_and_dispatch(&fixture.clock, 2_000_000_001);
    assert!(
        get_actor_unchecked::<OptionChainActor>(&fixture.actors[0])
            .slices
            .is_empty()
    );
    let quote = make_quote(instrument_id, "101", "103", 0);
    fixture.engine.borrow_mut().process_data(Data::Quote(quote));
    advance_clock_and_dispatch(&fixture.clock, 1_000_000_000);

    let greeks = OptionGreeks {
        ts_event: UnixNanos::from(20),
        ts_init: UnixNanos::from(25),
        ..make_option_chain_greeks(instrument_id, 50000.0)
    };

    fixture
        .engine
        .borrow_mut()
        .process_data(Data::OptionGreeks(greeks));
    advance_clock_and_dispatch(&fixture.clock, 1_000_000_000);
    advance_clock_and_dispatch(&fixture.clock, 1_000_000_000);

    let actor = get_actor_unchecked::<OptionChainActor>(&fixture.actors[0]);
    let timestamps: Vec<_> = actor
        .slices
        .iter()
        .map(|slice| (slice.ts_event.as_u64(), slice.ts_init.as_u64()))
        .collect();
    assert_eq!(
        timestamps,
        [(0, 3_000_000_001), (20, 4_000_000_001), (20, 5_000_000_001)]
    );
    drop(actor);
    fixture.dispose();
}

#[rstest]
#[case::raw(None)]
#[case::timed(Some(1_000))]
fn test_option_chain_actors_keep_distinct_slices(
    #[case] interval: Option<u64>,
    #[values(0, 1)] first_to_leave: usize,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let fixture = OptionChainFixture::new(clock, &cache);
    let strikes = [Price::from("50000.000"), Price::from("51000.000")];

    for index in 0..2 {
        fixture.subscribe(
            index,
            StrikeRange::Fixed(strikes[..=index].to_vec()),
            interval,
        );
    }

    let first_id = make_btc_option("50000.000", OptionKind::Call).id();
    let first_quote = make_quote(first_id, "101.000", "103.000", 11);
    fixture
        .engine
        .borrow_mut()
        .process_data(Data::Quote(first_quote));
    let added = make_btc_option("51000.000", OptionKind::Call);
    let added_quote = make_quote(added.id(), "211.000", "217.000", 13);
    fixture.engine.borrow_mut().process(&added);
    fixture
        .engine
        .borrow_mut()
        .process_data(Data::Quote(added_quote));

    if interval.is_some() {
        advance_clock_and_dispatch(&fixture.clock, 2_000_000_000);
    }

    for (index, id) in fixture.actors.iter().enumerate() {
        let actor = get_actor_unchecked::<OptionChainActor>(id);
        let slice = actor.slices.last().expect("actor receives its chain slice");
        assert_eq!(slice.series_id, make_series_id());
        assert_eq!(slice.atm_strike, None);
        assert_eq!(
            slice.calls.keys().copied().collect::<Vec<_>>(),
            strikes[..=index]
        );
        assert_eq!(slice.calls[&strikes[0]].quote, first_quote);
        assert_eq!(slice.calls[&strikes[0]].greeks, None);
        assert!(slice.puts.is_empty());

        if index == 1 {
            assert_eq!(slice.calls[&strikes[1]].quote, added_quote);
            assert_eq!(slice.calls[&strikes[1]].greeks, None);
        }
    }

    let before_leave: Vec<_> = fixture
        .actors
        .iter()
        .map(|id| get_actor_unchecked::<OptionChainActor>(id).slices.len())
        .collect();
    let commands = fixture.recorder.borrow();
    assert_eq!(commands.len(), 6);

    for command in commands.iter() {
        let DataCommand::Subscribe(command) = command else {
            panic!("expected child subscribe");
        };

        let instrument = command.instrument_ids().unwrap()[0];
        let index = usize::from(instrument == added.id());
        assert_eq!(command.client_id(), Some(&fixture.client_id));
        assert_eq!(command.params(), Some(&fixture.params(index)));
    }

    drop(commands);
    fixture.recorder.borrow_mut().clear();
    get_actor_unchecked::<OptionChainActor>(&fixture.actors[first_to_leave])
        .unsubscribe_option_chain(make_series_id(), Some(fixture.client_id));
    let survivor = 1 - first_to_leave;
    let updated_quote = make_quote(first_id, "307.000", "311.000", 17);
    fixture
        .engine
        .borrow_mut()
        .process_data(Data::Quote(updated_quote));

    if interval.is_some() {
        advance_clock_and_dispatch(&fixture.clock, 1_000_000_000);
    }

    assert_eq!(
        get_actor_unchecked::<OptionChainActor>(&fixture.actors[first_to_leave])
            .slices
            .len(),
        before_leave[first_to_leave]
    );
    let actor = get_actor_unchecked::<OptionChainActor>(&fixture.actors[survivor]);
    assert_eq!(actor.slices.len(), before_leave[survivor] + 1);
    assert_eq!(
        actor.slices.last().unwrap().calls[&strikes[0]].quote,
        updated_quote
    );
    drop(actor);
    assert!(
        fixture
            .engine
            .borrow()
            .subscribed_quotes()
            .contains(&first_id)
    );
    fixture.dispose();
    assert!(
        !fixture
            .engine
            .borrow()
            .has_option_chain_manager(&make_series_id())
    );
    assert_eq!(fixture.clock.borrow().timer_count(), 0);

    for instrument in [first_id, added.id()] {
        assert_eq!(
            msgbus::exact_subscriber_count_quotes(switchboard::get_quotes_topic(instrument)),
            0
        );
        assert_eq!(
            msgbus::exact_subscriber_count_option_greeks(switchboard::get_option_greeks_topic(
                instrument
            )),
            0
        );
    }

    let commands = fixture.recorder.borrow();
    assert_eq!(commands.len(), 6);
    assert_eq!(commands.iter().filter(|command| matches!(command,
        DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(command)) if command.instrument_ids == [first_id]
    )).count(), 1);
}

#[rstest]
#[case::reset(false)]
#[case::disconnect(true)]
fn test_option_chain_engine_cleanup_releases_every_owner(
    #[case] disconnect: bool,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let fixture = OptionChainFixture::new(clock, &cache);
    let instrument_id = make_btc_option("50000.000", OptionKind::Call).id();
    let second = make_btc_option("51000.000", OptionKind::Call);
    let second_id = second.id();
    fixture.engine.borrow_mut().process(&second);
    fixture.subscribe(
        0,
        StrikeRange::Fixed(vec![Price::from("50000.000"), Price::from("51000.000")]),
        Some(1_000),
    );
    fixture.subscribe(
        1,
        StrikeRange::AtmRelative {
            strikes_above: 1,
            strikes_below: 1,
        },
        Some(1_000),
    );

    assert_eq!(
        fixture.engine.borrow().pending_option_chain_request_count(),
        1
    );

    if disconnect {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        runtime
            .block_on(fixture.engine.borrow_mut().disconnect())
            .unwrap();
    } else {
        fixture.engine.borrow_mut().reset();
    }

    assert!(
        !fixture
            .engine
            .borrow()
            .has_option_chain_manager(&make_series_id())
    );
    assert_eq!(
        fixture.engine.borrow().pending_option_chain_request_count(),
        0
    );
    assert_eq!(fixture.clock.borrow().timer_count(), 0);
    assert!(fixture.engine.borrow().subscribed_quotes().is_empty());
    assert!(
        fixture
            .engine
            .borrow()
            .subscribed_instrument_status()
            .is_empty()
    );
    assert_eq!(
        msgbus::exact_subscriber_count_quotes(switchboard::get_quotes_topic(instrument_id)),
        0
    );
    assert_eq!(
        msgbus::exact_subscriber_count_option_greeks(switchboard::get_option_greeks_topic(
            instrument_id
        )),
        0
    );

    let released: Vec<_> = fixture
        .recorder
        .borrow()
        .iter()
        .filter_map(|command| match command {
            DataCommand::Unsubscribe(command) => Some(command.instrument_ids().unwrap()[0]),
            _ => None,
        })
        .collect();

    let mut expected_ids = [instrument_id, second_id];
    expected_ids.sort_unstable();
    let expected: Vec<_> = expected_ids.into_iter().flat_map(|id| [id; 3]).collect();
    assert_eq!(released, expected);
    fixture.dispose();
}

#[rstest]
#[case::reference_price(ChainBootstrap::Response(Some(Price::from("50000.000"))))]
#[case::greeks_fallback(ChainBootstrap::Response(None))]
#[case::timeout(ChainBootstrap::Timeout)]
fn test_option_chain_actors_bootstrap_independently(
    #[case] response: ChainBootstrap,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let fixture = OptionChainFixture::new(clock, &cache);
    for strike in ["49000.000", "51000.000"] {
        fixture
            .engine
            .borrow_mut()
            .process(&make_btc_option(strike, OptionKind::Call));
    }

    for index in 0..2 {
        fixture.subscribe(
            index,
            StrikeRange::AtmRelative {
                strikes_above: index,
                strikes_below: index,
            },
            None,
        );
    }

    let requests: Vec<_> = fixture
        .recorder
        .borrow()
        .iter()
        .filter_map(|command| match command {
            DataCommand::Request(RequestCommand::OptionChainReferencePrice(command)) => {
                Some(command.clone())
            }
            _ => None,
        })
        .collect();

    assert_eq!(requests.len(), 2);

    for (index, request) in requests.iter().enumerate() {
        assert_eq!(request.series_id, make_series_id());
        assert_eq!(request.client_id, Some(fixture.client_id));
        assert_eq!(request.params.as_ref(), Some(&fixture.params(index)));
    }

    match response {
        ChainBootstrap::Response(price) => {
            for request in &requests {
                fixture
                    .engine
                    .borrow_mut()
                    .response(DataResponse::OptionChainReferencePrice(
                        OptionChainReferencePriceResponse::new(
                            request.request_id,
                            fixture.client_id,
                            make_series_id(),
                            price,
                            UnixNanos::from(1),
                            None,
                        ),
                    ));
            }
        }
        ChainBootstrap::Timeout => {
            advance_clock_and_dispatch(&fixture.clock, 30_000_000_000);
            let commands = fixture.recorder.borrow();

            let first = commands
                .iter()
                .find_map(|command| match command {
                    DataCommand::Subscribe(SubscribeCommand::OptionGreeks(command)) => {
                        Some(command)
                    }
                    _ => None,
                })
                .expect("first bootstrap sample acquisition");

            assert_eq!(first.params.as_ref(), Some(&fixture.params(0)));
        }
    }

    let sample_id = make_btc_option("49000.000", OptionKind::Call).id();
    fixture
        .engine
        .borrow_mut()
        .process_data(Data::OptionGreeks(make_option_chain_greeks(
            sample_id, 50000.0,
        )));

    let expected = [
        Price::from("49000.000"),
        Price::from("50000.000"),
        Price::from("51000.000"),
    ];

    for (index, strike) in ["49000.000", "50000.000", "51000.000"]
        .into_iter()
        .enumerate()
    {
        let instrument_id = make_btc_option(strike, OptionKind::Call).id();
        fixture
            .engine
            .borrow_mut()
            .process_data(Data::Quote(make_quote(
                instrument_id,
                "101.000",
                "103.000",
                index as u64 + 3,
            )));
    }

    assert_eq!(
        fixture.engine.borrow().pending_option_chain_request_count(),
        0
    );
    assert_eq!(fixture.clock.borrow().timer_count(), 0);

    for (index, id) in fixture.actors.iter().enumerate() {
        let actor = get_actor_unchecked::<OptionChainActor>(id);
        let slice = actor
            .slices
            .last()
            .expect("bootstrapped actor receives its own slice");
        let strikes: Vec<_> = slice.calls.keys().copied().collect();
        assert_eq!(
            strikes,
            if index == 0 {
                expected[1..2].to_vec()
            } else {
                expected.to_vec()
            }
        );
    }

    fixture.dispose();
    assert!(fixture.engine.borrow().subscribed_quotes().is_empty());
    assert!(
        fixture
            .engine
            .borrow()
            .subscribed_instrument_status()
            .is_empty()
    );
    assert_eq!(
        msgbus::exact_subscriber_count_option_greeks(switchboard::get_option_greeks_topic(
            sample_id
        )),
        0
    );
}

#[rstest]
#[case::black_scholes_first([BlackScholes, PriceAdjusted], 0)]
#[case::price_adjusted_first([PriceAdjusted, BlackScholes], 1)]
fn test_option_chain_deferred_bootstrap_preserves_shared_greeks_coverage(
    #[case] conventions: [GreeksConvention; 2],
    #[case] first_to_bootstrap: usize,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let fixture = OptionChainFixture::new(clock, &cache);
    let sample = make_btc_option("49000.000", OptionKind::Call);
    let instrument_id = make_btc_option("50000.000", OptionKind::Call).id();
    fixture.engine.borrow_mut().process(&sample);

    let range = StrikeRange::AtmRelative {
        strikes_above: 0,
        strikes_below: 0,
    };

    fixture.subscribe(0, range.clone(), None);
    let mut params = fixture.params(1).to_shared_map();
    params.insert(
        "greeks_convention".to_string(),
        Arc::new(json!("PRICE_ADJUSTED")),
    );
    get_actor_unchecked::<OptionChainActor>(&fixture.actors[1])
        .subscribe_option_chain(
            make_series_id(),
            range,
            None,
            Some(fixture.client_id),
            Some(Params::from_shared(params)),
        )
        .unwrap();

    advance_clock_and_dispatch(&fixture.clock, 30_000_000_000);
    for convention in conventions {
        fixture.engine.borrow_mut().process(&OptionGreeks {
            convention,
            ..make_option_chain_greeks(sample.id(), 50000.0)
        });
    }

    let recorded: Vec<_> = fixture
        .recorder
        .borrow()
        .iter()
        .filter_map(|command| match command {
            DataCommand::Subscribe(SubscribeCommand::OptionGreeks(command)) => Some((
                command.single_instrument_id().unwrap(),
                command.client_id,
                command.params.clone(),
            )),
            _ => None,
        })
        .collect();

    assert_eq!(
        recorded,
        [
            (
                sample.id(),
                Some(fixture.client_id),
                Some(fixture.params(0))
            ),
            (
                instrument_id,
                Some(fixture.client_id),
                Some(fixture.params(first_to_bootstrap)),
            ),
        ],
    );

    let quote = make_quote(instrument_id, "101", "103", 2);
    fixture.engine.borrow_mut().process_data(Data::Quote(quote));
    let strike = Price::from("50000.000");

    for convention in conventions {
        fixture.engine.borrow_mut().process(&OptionGreeks {
            convention,
            ..make_option_chain_greeks(instrument_id, 50000.0)
        });
    }

    for (index, convention) in [BlackScholes, PriceAdjusted].into_iter().enumerate() {
        let actor = get_actor_unchecked::<OptionChainActor>(&fixture.actors[index]);
        assert_eq!(actor.slices.len(), 2);
        assert_eq!(actor.slices[1].calls[&strike].quote, quote);
        assert_eq!(
            actor.slices[1].get_call_greeks(&strike),
            Some(&OptionGreeks {
                convention,
                ..make_option_chain_greeks(instrument_id, 50000.0)
            }),
        );
    }

    fixture.dispose();
}

#[rstest]
#[case::quote(None, false)]
#[case::selected_greeks_typed(Some(BlackScholes), false)]
#[case::other_greeks_typed(Some(PriceAdjusted), false)]
#[case::selected_greeks_dynamic(Some(BlackScholes), true)]
#[case::other_greeks_dynamic(Some(PriceAdjusted), true)]
fn test_option_chain_queued_timer_expiry_precedes_delayed_data(
    #[case] convention: Option<GreeksConvention>,
    #[case] process_as_any: bool,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let series = make_series_id();
    let start = series.expiration_ns - DurationNanos::from_secs(2);
    clock.borrow_mut().advance_time(start, true);
    let fixture = OptionChainFixture::new(clock, &cache);
    let strike = Price::from("50000.000");
    let range = StrikeRange::Fixed(vec![strike]);
    fixture.subscribe(0, range.clone(), None);
    fixture.subscribe(1, range, Some(1_000));
    let instrument_id = make_btc_option("50000.000", OptionKind::Call).id();
    let quote = make_quote(instrument_id, "101", "103", start.as_u64());
    fixture.engine.borrow_mut().process_data(Data::Quote(quote));

    advance_clock_and_dispatch(&fixture.clock, 2_000_000_000);

    let before_expiry = series.expiration_ns - DurationNanos::new(1);

    {
        let mut engine = fixture.engine.borrow_mut();

        if let Some(convention) = convention {
            let greeks = OptionGreeks {
                convention,
                ts_event: before_expiry,
                ..make_option_chain_greeks(instrument_id, 50000.0)
            };

            if process_as_any {
                engine.process(&greeks);
            } else {
                engine.process_data(Data::OptionGreeks(greeks));
            }
        } else {
            let delayed_quote = make_quote(instrument_id, "102", "104", before_expiry.as_u64());
            engine.process_data(Data::Quote(delayed_quote));
        }
    }

    let actor = get_actor_unchecked::<OptionChainActor>(&fixture.actors[0]);
    assert_eq!(actor.slices.len(), 1);
    assert_eq!(actor.slices[0].calls[&strike].quote, quote);
    assert_eq!(actor.slices[0].get_call_greeks(&strike), None);
    assert!(!fixture.engine.borrow().has_option_chain_manager(&series));
    drop(actor);
    fixture.dispose();
}

#[rstest]
#[case::status(false)]
#[case::timer(true)]
fn test_option_chain_expiry_releases_all_owners(
    #[case] timer_expiry: bool,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    if timer_expiry {
        let start = make_series_id().expiration_ns - DurationNanos::from_secs(2);
        clock.borrow_mut().advance_time(start, true);
    }

    let fixture = OptionChainFixture::new(clock, &cache);
    let instrument_id = make_btc_option("50000.000", OptionKind::Call).id();

    for index in 0..2 {
        fixture.subscribe(
            index,
            StrikeRange::Fixed(vec![Price::from("50000.000")]),
            Some(1_000),
        );
    }

    fixture.recorder.borrow_mut().clear();
    if timer_expiry {
        advance_clock_and_dispatch(&fixture.clock, 2_000_000_000);
        let quote = make_quote(instrument_id, "101.000", "103.000", 1);
        fixture.engine.borrow_mut().process_data(Data::Quote(quote));
    } else {
        let status = InstrumentStatus::new(
            instrument_id,
            MarketStatusAction::Close,
            UnixNanos::from(3),
            UnixNanos::from(5),
            None,
            None,
            Some(false),
            Some(false),
            None,
        );

        fixture
            .engine
            .borrow_mut()
            .process_data(Data::InstrumentStatus(status));
    }

    assert!(
        !fixture
            .engine
            .borrow()
            .has_option_chain_manager(&make_series_id())
    );
    assert!(fixture.engine.borrow().subscribed_quotes().is_empty());
    assert!(
        fixture
            .engine
            .borrow()
            .subscribed_instrument_status()
            .is_empty()
    );
    assert_eq!(fixture.clock.borrow().timer_count(), 0);
    assert_eq!(fixture.recorder.borrow().len(), 3);
    assert_eq!(
        msgbus::exact_subscriber_count_quotes(switchboard::get_quotes_topic(instrument_id)),
        0
    );
    assert_eq!(
        msgbus::exact_subscriber_count_option_greeks(switchboard::get_option_greeks_topic(
            instrument_id
        )),
        0
    );
    fixture.dispose();
    assert_eq!(fixture.recorder.borrow().len(), 3);
}

#[rstest]
fn test_option_chain_retirement_preserves_direct_quotes(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let fixture = OptionChainFixture::new(clock, &cache);
    let instrument_id = make_btc_option("50000.000", OptionKind::Call).id();
    let direct_params = Params::new([("route".to_string(), json!(71))].into());
    let direct = SubscribeCommand::Quotes(SubscribeQuotes::new(
        instrument_id,
        Some(fixture.client_id),
        Some(instrument_id.venue),
        UUID4::new(),
        UnixNanos::from(1),
        None,
        Some(direct_params.clone()),
    ));
    fixture
        .engine
        .borrow_mut()
        .execute(DataCommand::Subscribe(direct.clone()));

    for index in 0..2 {
        fixture.subscribe(
            index,
            StrikeRange::Fixed(vec![Price::from("50000.000")]),
            None,
        );
    }

    fixture.recorder.borrow_mut().clear();
    fixture.dispose();
    assert!(
        !fixture
            .engine
            .borrow()
            .has_option_chain_manager(&make_series_id())
    );
    assert_eq!(
        fixture.engine.borrow().subscribed_quotes(),
        vec![instrument_id]
    );
    assert!(
        fixture
            .engine
            .borrow()
            .subscribed_instrument_status()
            .is_empty()
    );
    assert_eq!(fixture.recorder.borrow().len(), 2);
    let unsubscribe = direct.into_unsubscribe(UUID4::new(), UnixNanos::from(2), None);
    fixture
        .engine
        .borrow_mut()
        .execute(DataCommand::Unsubscribe(unsubscribe));
    assert!(fixture.engine.borrow().subscribed_quotes().is_empty());
    let commands = fixture.recorder.borrow();

    let DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(command)) = commands.last().unwrap()
    else {
        panic!("expected final direct quote unsubscribe");
    };

    assert_eq!(command.instrument_ids, vec![instrument_id]);
    assert_eq!(command.client_id, Some(fixture.client_id));
    assert_eq!(command.params.as_ref(), Some(&direct_params));
}

#[derive(Debug, Clone, Copy)]
enum ChainBootstrap {
    Response(Option<Price>),
    Timeout,
}

#[derive(Debug)]
struct OptionChainActor {
    core: DataActorCore,
    slices: Vec<OptionChainSlice>,
}

nautilus_actor!(OptionChainActor);

impl DataActor for OptionChainActor {
    fn on_option_chain(&mut self, slice: &OptionChainSlice) -> anyhow::Result<()> {
        self.slices.push(slice.clone());
        Ok(())
    }
}

#[rstest]
fn test_option_chain_cache_only_close_preserves_unlisted_owner(
    #[values(false, true)] active_peer: bool,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let fixture = OptionChainFixture::new(clock, &cache);
    let closed = make_btc_option("50000.000", OptionKind::Call);
    let listing = make_btc_option("51000.000", OptionKind::Call);
    cache.borrow_mut().purge_instrument(closed.id());
    fixture.subscribe(0, StrikeRange::Fixed(vec![Price::from("51000.000")]), None);
    for instrument in [closed.clone(), listing.clone()] {
        fixture
            .engine
            .borrow_mut()
            .response(DataResponse::Instrument(Box::new(InstrumentResponse::new(
                UUID4::new(),
                fixture.client_id,
                instrument.id(),
                instrument,
                None,
                None,
                UnixNanos::from(1),
                None,
            ))));
    }

    if active_peer {
        fixture.subscribe(1, StrikeRange::Fixed(vec![Price::from("50000.000")]), None);
    }

    fixture
        .engine
        .borrow_mut()
        .process_data(Data::InstrumentStatus(InstrumentStatus::new(
            closed.id(),
            MarketStatusAction::Close,
            UnixNanos::from(2),
            UnixNanos::from(3),
            None,
            None,
            Some(false),
            None,
            None,
        )));
    fixture.recorder.borrow_mut().clear();
    fixture.engine.borrow_mut().process(&listing);
    let quote = make_quote(listing.id(), "201.000", "203.000", 5);
    fixture.engine.borrow_mut().process_data(Data::Quote(quote));

    let actor = get_actor_unchecked::<OptionChainActor>(&fixture.actors[0]);
    assert_eq!(actor.slices.len(), 1);
    assert_eq!(
        actor.slices[0].calls[&Price::from("51000.000")].quote,
        quote
    );
    drop(actor);
    assert_eq!(fixture.engine.borrow().subscribed_quotes(), [listing.id()]);
    assert_eq!(
        fixture.engine.borrow().subscribed_instrument_status(),
        [listing.id()]
    );
    assert_eq!(fixture.recorder.borrow().len(), 3);
    fixture.dispose();
    assert_eq!(fixture.recorder.borrow().len(), 6);
    assert!(fixture.engine.borrow().subscribed_quotes().is_empty());
    assert!(
        fixture
            .engine
            .borrow()
            .subscribed_instrument_status()
            .is_empty()
    );
    assert!(
        fixture.engine.borrow().get_clients()[0]
            .subscriptions_option_greeks
            .is_empty()
    );
    assert_eq!(
        msgbus::exact_subscriber_count_quotes(switchboard::get_quotes_topic(listing.id())),
        0
    );
    assert_eq!(
        msgbus::exact_subscriber_count_option_greeks(switchboard::get_option_greeks_topic(
            listing.id()
        )),
        0
    );
}

#[rstest]
fn test_option_chain_pending_retirement_uses_cached_catalog(
    #[values(false, true)] active_owner: bool,
    #[values(false, true)] remaining_contract: bool,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let fixture = OptionChainFixture::new(clock, &cache);
    if active_owner {
        fixture.subscribe(0, StrikeRange::Fixed(vec![Price::from("50000.000")]), None);
    }

    let open = make_btc_option("51000.000", OptionKind::Call);

    if remaining_contract {
        fixture
            .engine
            .borrow_mut()
            .response(DataResponse::Instrument(Box::new(InstrumentResponse::new(
                UUID4::new(),
                fixture.client_id,
                open.id(),
                open.clone(),
                None,
                None,
                UnixNanos::from(1),
                None,
            ))));
    }

    fixture.subscribe(
        1,
        StrikeRange::AtmRelative {
            strikes_above: 1,
            strikes_below: 1,
        },
        Some(1_000),
    );

    let request_id = option_chain_reference_price_request_id(&fixture.recorder);
    let closed = make_btc_option("50000.000", OptionKind::Call).id();
    fixture
        .engine
        .borrow_mut()
        .process_data(Data::InstrumentStatus(InstrumentStatus::new(
            closed,
            MarketStatusAction::Close,
            UnixNanos::from(2),
            UnixNanos::from(3),
            None,
            None,
            Some(false),
            None,
            None,
        )));
    assert_eq!(
        fixture.engine.borrow().pending_option_chain_request_count(),
        usize::from(remaining_contract)
    );
    fixture.recorder.borrow_mut().clear();
    fixture
        .engine
        .borrow_mut()
        .response(DataResponse::OptionChainReferencePrice(
            OptionChainReferencePriceResponse::new(
                request_id,
                fixture.client_id,
                make_series_id(),
                Some(Price::from("51000.000")),
                UnixNanos::from(4),
                None,
            ),
        ));
    fixture
        .engine
        .borrow_mut()
        .process_data(Data::OptionGreeks(make_option_chain_greeks(
            open.id(),
            51000.0,
        )));
    let quote = make_quote(open.id(), "201.000", "203.000", 5);
    fixture.engine.borrow_mut().process_data(Data::Quote(quote));
    advance_clock_and_dispatch(&fixture.clock, 2_000_000_000);

    assert_eq!(
        fixture
            .engine
            .borrow()
            .has_option_chain_manager(&make_series_id()),
        remaining_contract
    );
    assert_eq!(
        fixture.clock.borrow().timer_count(),
        usize::from(remaining_contract)
    );
    let actor = get_actor_unchecked::<OptionChainActor>(&fixture.actors[1]);

    if remaining_contract {
        assert_eq!(actor.slices.len(), 1);
        assert_eq!(
            actor.slices[0].calls[&Price::from("51000.000")].quote,
            quote
        );
        assert_eq!(fixture.engine.borrow().subscribed_quotes(), [open.id()]);
        assert_eq!(fixture.recorder.borrow().len(), 3);
    } else {
        assert!(actor.slices.is_empty());
        assert!(fixture.engine.borrow().subscribed_quotes().is_empty());
        assert!(fixture.recorder.borrow().is_empty());
    }

    drop(actor);
    fixture.dispose();
    assert!(fixture.engine.borrow().subscribed_quotes().is_empty());
    assert!(
        fixture
            .engine
            .borrow()
            .subscribed_instrument_status()
            .is_empty()
    );
    assert_eq!(fixture.clock.borrow().timer_count(), 0);
}

#[rstest]
fn test_option_chain_backtest_registration_preserves_acquired_client(
    #[values(false, true)] dynamic: bool,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let fixture = OptionChainFixture::new(Rc::clone(&clock), &cache);
    let range = StrikeRange::Fixed(vec![Price::from("50000.000"), Price::from("51000.000")]);
    fixture.subscribe(0, range.clone(), None);
    let backtest = ClientId::from("BACKTEST");
    let recorder = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        backtest,
        Venue::from("DERIBIT"),
        None,
        &recorder,
        &mut fixture.engine.borrow_mut(),
    );
    fixture.subscribe(1, range, None);
    if dynamic {
        fixture
            .engine
            .borrow_mut()
            .process(&make_btc_option("51000.000", OptionKind::Call));
    }

    fixture.recorder.borrow_mut().clear();
    recorder.borrow_mut().clear();
    get_actor_unchecked::<OptionChainActor>(&fixture.actors[0])
        .unsubscribe_option_chain(make_series_id(), None);
    assert!(recorder.borrow().is_empty());

    let expected = if dynamic { 6 } else { 3 };
    assert_eq!(fixture.recorder.borrow().len(), expected);

    for client in fixture.engine.borrow().get_clients() {
        assert_eq!(
            client.subscriptions_quotes.len(),
            if client.client_id() == backtest {
                expected / 3
            } else {
                0
            }
        );
        assert_eq!(
            client.subscriptions_option_greeks.len(),
            if client.client_id() == backtest {
                expected / 3
            } else {
                0
            }
        );
        assert_eq!(
            client.subscriptions_instrument_status.len(),
            if client.client_id() == backtest {
                expected / 3
            } else {
                0
            }
        );
    }

    let quote = make_quote(
        make_btc_option("50000.000", OptionKind::Call).id(),
        "101.000",
        "103.000",
        7,
    );
    fixture.engine.borrow_mut().process_data(Data::Quote(quote));
    assert_eq!(
        get_actor_unchecked::<OptionChainActor>(&fixture.actors[1])
            .slices
            .last()
            .unwrap()
            .calls[&Price::from("50000.000")]
            .quote,
        quote
    );
    fixture.dispose();
    assert_eq!(recorder.borrow().len(), expected);
    assert!(fixture.engine.borrow().subscribed_quotes().is_empty());
}

#[rstest]
fn test_option_chain_expired_series_rejects_late_definitions(
    #[values(false, true)] dynamic: bool,
    #[values(None, Some(1_000))] interval: Option<u64>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let fixture = OptionChainFixture::new(clock, &cache);
    fixture.subscribe(0, StrikeRange::Fixed(vec![Price::from("50000.000")]), None);
    fixture
        .clock
        .borrow_mut()
        .advance_time(make_series_id().expiration_ns, true);
    fixture
        .engine
        .borrow_mut()
        .process_data(Data::Quote(make_quote(
            make_btc_option("50000.000", OptionKind::Call).id(),
            "101.000",
            "103.000",
            make_series_id().expiration_ns.as_u64(),
        )));
    let late = make_btc_option("51000.000", OptionKind::Call);
    fixture
        .engine
        .borrow_mut()
        .response(DataResponse::Instrument(Box::new(InstrumentResponse::new(
            UUID4::new(),
            fixture.client_id,
            late.id(),
            late,
            None,
            None,
            make_series_id().expiration_ns,
            None,
        ))));
    fixture.recorder.borrow_mut().clear();
    fixture.subscribe(
        1,
        if dynamic {
            StrikeRange::AtmRelative {
                strikes_above: 1,
                strikes_below: 1,
            }
        } else {
            StrikeRange::Fixed(vec![Price::from("51000.000")])
        },
        interval,
    );

    assert!(fixture.recorder.borrow().is_empty());
    assert!(
        !fixture
            .engine
            .borrow()
            .has_option_chain_manager(&make_series_id())
    );
    assert_eq!(
        fixture.engine.borrow().pending_option_chain_request_count(),
        0
    );
    assert_eq!(fixture.clock.borrow().timer_count(), 0);
    assert!(fixture.engine.borrow().subscribed_quotes().is_empty());
    assert!(
        fixture
            .engine
            .borrow()
            .subscribed_instrument_status()
            .is_empty()
    );
    fixture.dispose();
}

#[rstest]
fn test_option_chain_expired_series_ignores_pending_response(
    #[values(None, Some(Price::from("50000.000")))] price: Option<Price>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let fixture = OptionChainFixture::new(clock, &cache);
    fixture.subscribe(
        0,
        StrikeRange::AtmRelative {
            strikes_above: 1,
            strikes_below: 1,
        },
        Some(1_000),
    );

    let request_id = option_chain_reference_price_request_id(&fixture.recorder);
    fixture
        .clock
        .borrow_mut()
        .advance_time(make_series_id().expiration_ns, true);
    fixture.recorder.borrow_mut().clear();
    fixture
        .engine
        .borrow_mut()
        .response(DataResponse::OptionChainReferencePrice(
            OptionChainReferencePriceResponse::new(
                request_id,
                fixture.client_id,
                make_series_id(),
                price,
                make_series_id().expiration_ns,
                None,
            ),
        ));

    assert!(fixture.recorder.borrow().is_empty());
    assert!(
        !fixture
            .engine
            .borrow()
            .has_option_chain_manager(&make_series_id())
    );
    assert_eq!(
        fixture.engine.borrow().pending_option_chain_request_count(),
        0
    );
    assert_eq!(fixture.clock.borrow().timer_count(), 0);
    assert!(fixture.engine.borrow().subscribed_quotes().is_empty());
    assert!(
        fixture
            .engine
            .borrow()
            .subscribed_instrument_status()
            .is_empty()
    );
    assert!(
        fixture.engine.borrow().get_clients()[0]
            .subscriptions_option_greeks
            .is_empty()
    );
    fixture.dispose();
}

#[rstest]
fn test_option_chain_fixed_range_first_listing_acquires_once(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let fixture = OptionChainFixture::new(clock, &cache);
    fixture.subscribe(0, StrikeRange::Fixed(vec![Price::from("51000.000")]), None);
    assert!(fixture.recorder.borrow().is_empty());
    let listing = make_btc_option("51000.000", OptionKind::Call);
    fixture.engine.borrow_mut().process(&listing);
    fixture
        .engine
        .borrow_mut()
        .process_data(Data::OptionGreeks(make_option_chain_greeks(
            listing.id(),
            51000.0,
        )));
    let quote = make_quote(listing.id(), "201.000", "203.000", 5);
    fixture.engine.borrow_mut().process_data(Data::Quote(quote));
    assert_eq!(
        get_actor_unchecked::<OptionChainActor>(&fixture.actors[0])
            .slices
            .last()
            .unwrap()
            .calls[&Price::from("51000.000")]
            .quote,
        quote
    );
    assert_eq!(fixture.recorder.borrow().len(), 3);
    fixture.dispose();
    assert_eq!(fixture.recorder.borrow().len(), 6);
    assert!(fixture.engine.borrow().subscribed_quotes().is_empty());
    assert!(
        fixture
            .engine
            .borrow()
            .subscribed_instrument_status()
            .is_empty()
    );
    assert!(
        fixture.engine.borrow().get_clients()[0]
            .subscriptions_option_greeks
            .is_empty()
    );
    assert_eq!(
        msgbus::exact_subscriber_count_quotes(switchboard::get_quotes_topic(listing.id())),
        0
    );
    assert_eq!(
        msgbus::exact_subscriber_count_option_greeks(switchboard::get_option_greeks_topic(
            listing.id()
        )),
        0
    );
}

struct OptionChainFixture {
    clock: Rc<RefCell<VirtualClock>>,
    engine: Rc<RefCell<DataEngine>>,
    recorder: Rc<RefCell<Vec<DataCommand>>>,
    actors: [Ustr; 2],
    client_id: ClientId,
}

impl OptionChainFixture {
    fn new(clock: Rc<RefCell<VirtualClock>>, cache: &Rc<RefCell<Cache>>) -> Self {
        let _bus = MessageBus::new(TraderId::test_default(), UUID4::new(), None, None)
            .register_message_bus();
        set_data_cmd_sender(Arc::new(SyncDataCommandSender));
        let engine = make_option_chain_engine(Rc::clone(&clock), Rc::clone(cache));
        let target = Rc::clone(&engine);
        msgbus::register_data_command_endpoint(
            MessagingSwitchboard::data_engine_queue_execute(),
            TypedIntoHandler::from(move |command: DataCommand| {
                target.borrow_mut().execute(command);
            }),
        );

        let client_id = ClientId::from("DERIBIT-CHAIN");
        let recorder = Rc::new(RefCell::new(Vec::new()));
        register_mock_client(
            Rc::clone(&clock),
            Rc::clone(cache),
            client_id,
            Venue::from("DERIBIT"),
            Some(Venue::from("DERIBIT")),
            &recorder,
            &mut engine.borrow_mut(),
        );
        cache
            .borrow_mut()
            .add_instrument(make_btc_option("50000.000", OptionKind::Call))
            .unwrap();

        let actors = std::array::from_fn(|index| {
            let mut actor = OptionChainActor {
                core: DataActorCore::new(DataActorConfig {
                    actor_id: Some(ActorId::from(format!("CHAIN-{index}"))),
                    ..Default::default()
                }),
                slices: Vec::new(),
            };

            actor
                .register(
                    TraderId::test_default(),
                    Rc::clone(&clock) as Rc<RefCell<dyn Clock>>,
                    Rc::clone(cache),
                )
                .unwrap();
            actor.start().unwrap();
            let id = actor.actor_id().inner();
            register_actor(actor);
            id
        });

        Self {
            clock,
            engine,
            recorder,
            actors,
            client_id,
        }
    }

    fn params(&self, index: usize) -> Params {
        Params::new([("route".to_string(), json!(index + 23))].into())
    }

    fn subscribe(&self, index: usize, range: StrikeRange, interval: Option<u64>) {
        get_actor_unchecked::<OptionChainActor>(&self.actors[index])
            .subscribe_option_chain(
                make_series_id(),
                range,
                interval,
                Some(self.client_id),
                Some(self.params(index)),
            )
            .unwrap();
    }

    fn resolve_reference_price(&self, price: Price) {
        let request_id = option_chain_reference_price_request_id(&self.recorder);
        self.engine
            .borrow_mut()
            .response(DataResponse::OptionChainReferencePrice(
                OptionChainReferencePriceResponse::new(
                    request_id,
                    self.client_id,
                    make_series_id(),
                    Some(price),
                    UnixNanos::from(1),
                    None,
                ),
            ));
    }

    fn dispose(&self) {
        for id in &self.actors {
            let mut actor = get_actor_unchecked::<OptionChainActor>(id);
            actor.stop().unwrap();
            actor.dispose().unwrap();
            drop(actor);
            deregister_actor(id);
        }
    }
}

fn option_chain_unsubscribe(command: &DataCommand) -> DataCommand {
    let DataCommand::Subscribe(command) = command else {
        panic!("expected subscribe command");
    };

    DataCommand::Unsubscribe(command.clone().into_unsubscribe(
        UUID4::new(),
        UnixNanos::default(),
        None,
    ))
}

fn make_subscribe_option_chain(
    series_id: OptionSeriesId,
    strikes: Vec<Price>,
    client_id: Option<ClientId>,
    venue: Option<Venue>,
) -> DataCommand {
    DataCommand::Subscribe(SubscribeCommand::OptionChain(
        SubscribeOptionChain::new(
            series_id,
            StrikeRange::Fixed(strikes),
            Some(1000),
            UUID4::new(),
            UnixNanos::default(),
            client_id,
            venue,
            None,
        )
        .unwrap(),
    ))
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
    DataCommand::Subscribe(SubscribeCommand::OptionChain(
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
        )
        .unwrap(),
    ))
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

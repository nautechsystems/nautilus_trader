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
#[should_panic]
fn test_register_default_client_twice_panics(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
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
            Rc::clone(&clock) as Rc<RefCell<dyn Clock>>,
            Rc::clone(&cache),
            client_id,
            Some(Venue::test_default()),
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
            Some(Venue::test_default()),
        )),
    );

    data_engine.register_default_client(data_client1);
    data_engine.register_default_client(data_client2);
}

#[rstest]
#[should_panic]
fn test_register_client_duplicate_id_panics(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let mut data_engine = data_engine.borrow_mut();

    let client_id = ClientId::new("DUPLICATE");
    let venue = Venue::test_default();

    let data_client1 = DataClientAdapter::new(
        client_id,
        Some(venue),
        true,
        true,
        Box::new(MockDataClient::new(
            Rc::clone(&clock) as Rc<RefCell<dyn Clock>>,
            Rc::clone(&cache),
            client_id,
            Some(Venue::test_default()),
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
            Some(Venue::test_default()),
        )),
    );

    data_engine.register_client(data_client1, None);
    data_engine.register_client(data_client2, None);
}

#[rstest]
fn test_register_and_deregister_client(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let mut data_engine = data_engine.borrow_mut();

    let client_id1 = ClientId::new("C1");
    let venue1 = Venue::test_default();

    let data_client1 = DataClientAdapter::new(
        client_id1,
        Some(venue1),
        true,
        true,
        Box::new(MockDataClient::new(
            Rc::clone(&clock) as Rc<RefCell<dyn Clock>>,
            Rc::clone(&cache),
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
    clock: Rc<RefCell<VirtualClock>>,
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
            Some(Venue::test_default()),
        )),
    );
    data_engine.register_default_client(default_client);

    assert_eq!(data_engine.registered_clients(), vec![default_id]);
    assert_eq!(
        data_engine.get_client(None, None).unwrap().client_id(),
        default_id
    );
}

#[rstest]
fn test_register_venue_routing_routes_exchange_venue_to_client(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let mut data_engine = data_engine.borrow_mut();

    let broker_client_id = ClientId::new("IB");
    let exchange_venue = Venue::new("IBIS");
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let client = MockDataClient::new_with_recorder(
        clock,
        cache,
        broker_client_id,
        None,
        Some(Rc::clone(&recorder)),
    );
    let adapter = DataClientAdapter::new(broker_client_id, None, true, true, Box::new(client));
    data_engine.register_client(adapter, None);
    data_engine
        .register_venue_routing(broker_client_id, exchange_venue)
        .unwrap();

    let instrument_id = InstrumentId::new(Symbol::new("VWCE"), exchange_venue);
    let sub_cmd = DataCommand::Subscribe(SubscribeCommand::BookDeltas(SubscribeBookDeltas::new(
        instrument_id,
        BookType::L3_MBO,
        None,
        Some(exchange_venue),
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
fn test_default_and_venue_routing_apply_independently_for_venue_less_client(
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    let mut data_engine = data_engine.borrow_mut();

    let broker_client_id = ClientId::new("IB");
    let exchange_venue = Venue::new("IBIS");
    let recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let client = MockDataClient::new_with_recorder(
        clock,
        cache,
        broker_client_id,
        None,
        Some(Rc::clone(&recorder)),
    );
    let adapter = DataClientAdapter::new(broker_client_id, None, true, true, Box::new(client));
    data_engine.register_client(adapter, None);
    data_engine.set_default_client(broker_client_id).unwrap();
    data_engine
        .register_venue_routing(broker_client_id, exchange_venue)
        .unwrap();

    let routed_id = InstrumentId::new(Symbol::new("VWCE"), exchange_venue);
    let routed_cmd =
        DataCommand::Subscribe(SubscribeCommand::BookDeltas(SubscribeBookDeltas::new(
            routed_id,
            BookType::L3_MBO,
            None,
            Some(exchange_venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            true,
            None,
            None,
        )));
    data_engine.execute(routed_cmd.clone());

    let unmapped_venue = Venue::new("UNKNOWN");
    let default_cmd =
        DataCommand::Subscribe(SubscribeCommand::BookDeltas(SubscribeBookDeltas::new(
            InstrumentId::new(Symbol::new("XYZ"), unmapped_venue),
            BookType::L3_MBO,
            None,
            Some(unmapped_venue),
            UUID4::new(),
            UnixNanos::default(),
            None,
            true,
            None,
            None,
        )));
    data_engine.execute(default_cmd.clone());

    assert_eq!(recorder.borrow().as_slice(), &[routed_cmd, default_cmd]);
}

#[rstest]
fn test_backtest_client_overrides_subscribe_routing(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();

    let venue_client_id = ClientId::new("VENUE_LIVE");
    let backtest_client_id = ClientId::new("BACKTEST");

    let venue_recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        Rc::clone(&clock),
        Rc::clone(&cache),
        venue_client_id,
        venue,
        None,
        &venue_recorder,
        &mut data_engine,
    );

    let backtest_recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        clock,
        cache,
        backtest_client_id,
        venue,
        None,
        &backtest_recorder,
        &mut data_engine,
    );

    let sub = DataCommand::Subscribe(SubscribeCommand::Quotes(SubscribeQuotes::new(
        audusd_sim.id,
        Some(venue_client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )));
    data_engine.execute(sub);

    assert_eq!(
        backtest_recorder.borrow().len(),
        1,
        "BACKTEST client should receive the subscribe override",
    );
    assert!(
        venue_recorder.borrow().is_empty(),
        "venue client should not receive subscribes when BACKTEST is registered",
    );
}

#[rstest]
fn test_backtest_client_overrides_when_registered_as_default(
    audusd_sim: CurrencyPair,
    data_engine: Rc<RefCell<DataEngine>>,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    venue: Venue,
) {
    let mut data_engine = data_engine.borrow_mut();

    let venue_client_id = ClientId::new("VENUE_LIVE");
    let backtest_client_id = ClientId::new("BACKTEST");

    let venue_recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    register_mock_client(
        Rc::clone(&clock),
        Rc::clone(&cache),
        venue_client_id,
        venue,
        None,
        &venue_recorder,
        &mut data_engine,
    );

    // `BacktestEngine` registers BACKTEST with venue=None, which lands the
    // adapter in `default_client` rather than `clients`
    let backtest_recorder: Rc<RefCell<Vec<DataCommand>>> = Rc::new(RefCell::new(Vec::new()));
    let backtest = MockDataClient::new_with_recorder(
        clock,
        cache,
        backtest_client_id,
        None,
        Some(Rc::clone(&backtest_recorder)),
    );
    let backtest_adapter =
        DataClientAdapter::new(backtest_client_id, None, true, true, Box::new(backtest));
    data_engine.register_client(backtest_adapter, None);

    let sub = DataCommand::Subscribe(SubscribeCommand::Quotes(SubscribeQuotes::new(
        audusd_sim.id,
        Some(venue_client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )));
    data_engine.execute(sub);

    assert_eq!(
        backtest_recorder.borrow().len(),
        1,
        "BACKTEST default client must receive subscribes",
    );
    assert!(
        venue_recorder.borrow().is_empty(),
        "venue client must not receive subscribes when BACKTEST is the default",
    );
}

#[rstest]
fn test_external_client_forwards_subscribe_and_unsubscribe_commands(
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

    let subscribe = SubscribeCommand::Quotes(SubscribeQuotes::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::from(1),
        None,
        None,
    ));
    let (subscribe_handler, subscribe_saver) = get_any_saving_handler::<SubscribeCommand>(None);
    msgbus::subscribe_any(topic.as_str().into(), subscribe_handler.clone(), None);

    data_engine.execute(DataCommand::Subscribe(subscribe.clone()));

    msgbus::unsubscribe_any(topic.as_str().into(), &subscribe_handler);
    assert_eq!(
        serde_json::to_value(subscribe_saver.get_messages()).unwrap(),
        serde_json::to_value([subscribe]).unwrap(),
    );

    let unsubscribe = UnsubscribeCommand::Quotes(UnsubscribeQuotes::new(
        audusd_sim.id,
        Some(client_id),
        Some(venue),
        UUID4::new(),
        UnixNanos::from(2),
        None,
        None,
    ));
    let (unsubscribe_handler, unsubscribe_saver) =
        get_any_saving_handler::<UnsubscribeCommand>(None);
    msgbus::subscribe_any(topic.as_str().into(), unsubscribe_handler, None);

    data_engine.execute(DataCommand::Unsubscribe(unsubscribe.clone()));

    assert_eq!(
        serde_json::to_value(unsubscribe_saver.get_messages()).unwrap(),
        serde_json::to_value([unsubscribe]).unwrap(),
    );
}

#[rstest]
#[tokio::test]
#[expect(clippy::await_holding_refcell_ref)] // Single-threaded test
async fn test_data_engine_connect_continues_with_failing_client(
    #[from(data_engine)] data_engine: Rc<RefCell<DataEngine>>,
) {
    let mut data_engine = data_engine.borrow_mut();

    let client_id = ClientId::from("FAILING_CLIENT");
    let venue = Venue::from("TEST");
    let error_message = "Authentication failed: invalid API key";

    let client = FailingMockDataClient::new(client_id, Some(venue), error_message);
    let adapter = DataClientAdapter::new(client_id, Some(venue), true, true, Box::new(client));
    data_engine.register_client(adapter, None);

    // Connect logs errors but does not fail
    data_engine.connect().await;
}

#[rstest]
#[tokio::test]
#[expect(clippy::await_holding_refcell_ref)] // Single-threaded test
async fn test_data_engine_connect_succeeds_with_working_client(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    #[from(data_engine)] data_engine: Rc<RefCell<DataEngine>>,
) {
    let mut data_engine = data_engine.borrow_mut();

    let client_id = ClientId::from("WORKING_CLIENT");
    let venue = Venue::from("TEST");

    let client = MockDataClient::new(clock, cache, client_id, Some(venue));
    let adapter = DataClientAdapter::new(client_id, Some(venue), true, true, Box::new(client));
    data_engine.register_client(adapter, None);

    data_engine.connect().await;
}

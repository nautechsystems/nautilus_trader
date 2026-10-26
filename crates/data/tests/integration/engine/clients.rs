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

use std::sync::Once;

use log::{Level, LevelFilter, Log, Metadata, Record};
#[cfg(feature = "defi")]
use nautilus_common::messages::defi::{SubscribeBlocks, UnsubscribeBlocks};

use super::*;

thread_local! {
    static CONNECTION_LOGS: RefCell<Vec<(Level, String)>> = const { RefCell::new(Vec::new()) };
}

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
    #[values(0, 1, 2)] client_index: usize,
    audusd_sim: CurrencyPair,
    _stub_msgbus: Rc<RefCell<MessageBus>>,
    venue: Venue,
) {
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache: Rc<RefCell<Cache>> = Rc::new(RefCell::new(Cache::default()));
    let client_ids = [
        ClientId::from("EXTERNAL-ALPHA"),
        ClientId::from("EXTERNAL-BRAVO"),
        ClientId::from("EXTERNAL-PREVIOUS"),
    ];
    let client_id = client_ids[client_index];
    cache.borrow_mut().add_external_data_client(client_ids[2]);

    let config = DataEngineConfig {
        external_clients: Some(client_ids[..2].to_vec()),
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

#[rstest]
#[case("password=SYN-password token=SYN-token account=SYN-account")]
#[case(
    "https://SYN-user:SYN-password@example.invalid/?token=SYN%2Dtoken\nAuthorization: Bearer SYN-bearer"
)]
#[tokio::test]
async fn test_data_engine_connect_omits_provider_errors(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    #[case] error_message: &'static str,
    #[values(false, true)] failing_first: bool,
) {
    capture_connection_logs();

    log::error!(target: "nautilus_data::engine", "Connection log capture control");
    assert_eq!(
        take_connection_logs(),
        vec![(Level::Error, "Connection log capture control".to_string())],
    );

    let mut data_engine = DataEngine::new(clock, cache, None);
    let failing_id = ClientId::from("FAILING_CLIENT");
    let working_id = ClientId::from("WORKING_CLIENT");
    let barrier = Rc::new(tokio::sync::Barrier::new(2));

    let failing = ConnectionDataClient {
        client_id: failing_id,
        error_message: Some(error_message),
        error_operation: None,
        barrier: Rc::clone(&barrier),
        connected: false,
        calls: Rc::new(RefCell::new(Vec::new())),
    };

    let working = ConnectionDataClient {
        client_id: working_id,
        error_message: None,
        error_operation: None,
        barrier,
        connected: false,
        calls: Rc::new(RefCell::new(Vec::new())),
    };

    let failing = DataClientAdapter::new(failing_id, None, true, true, Box::new(failing));
    let working = DataClientAdapter::new(working_id, None, true, true, Box::new(working));

    if failing_first {
        data_engine.register_client(failing, None);
        data_engine.register_client(working, None);
    } else {
        data_engine.register_client(working, None);
        data_engine.register_client(failing, None);
    }

    take_connection_logs();

    tokio::time::timeout(std::time::Duration::from_secs(1), data_engine.connect())
        .await
        .expect("clients must connect concurrently");

    assert_eq!(
        take_connection_logs(),
        vec![(
            Level::Error,
            "Failed to connect data client FAILING_CLIENT: connection failed".to_string(),
        )],
    );
    assert!(
        data_engine
            .get_client(Some(&working_id), None)
            .unwrap()
            .is_connected()
    );
    assert!(
        data_engine
            .get_client(Some(&failing_id), None)
            .unwrap()
            .is_disconnected()
    );
    assert!(!data_engine.check_connected());
}

#[rstest]
#[case::instruments(false)]
#[case::bars(true)]
fn test_partial_subscription_logs_only_failed_members(
    #[case] bars: bool,
    #[values(false, true)] release: bool,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    client_id: ClientId,
    venue: Venue,
) {
    let first = InstrumentId::from("AUD/USD.SIM");
    let second = InstrumentId::from("GBP/USD.SIM");
    let foreign = InstrumentId::from("FOREIGN.SIM");
    let first_bar = BarType::from("AUD/USD.SIM-1-MINUTE-LAST-EXTERNAL");
    let second_bar = BarType::from("AUD/USD.SIM-5-MINUTE-LAST-EXTERNAL");
    let foreign_bar = BarType::from("AUD/USD.SIM-1-HOUR-LAST-EXTERNAL");
    let outcome = if bars {
        MockSubscribeFailure::BarsPartial([first_bar, first_bar, foreign_bar])
    } else {
        MockSubscribeFailure::MarkPricesPartial([first, first, foreign])
    };

    let mut client =
        MockDataClient::new(clock, cache, client_id, Some(venue)).with_unsubscribe_failure(outcome);
    if !release {
        client = client.with_subscribe_failure(outcome);
    }

    let mut adapter =
        DataClientAdapter::new(client_id, Some(venue), false, false, Box::new(client));
    let command_id = UUID4::new();
    let params = Some(params_from_json(json!({"private": "must-not-log"})));

    let command = if bars {
        SubscribeCommand::Bars(SubscribeBars::new(
            [first_bar, second_bar],
            Some(client_id),
            Some(venue),
            command_id,
            UnixNanos::from(31),
            None,
            params,
        ))
    } else {
        SubscribeCommand::MarkPrices(SubscribeMarkPrices::new(
            [first, second],
            Some(client_id),
            Some(venue),
            command_id,
            UnixNanos::from(31),
            None,
            params,
        ))
    };

    capture_connection_logs();
    adapter.execute_subscribe(command.clone());

    let (direction, outcome, operation_id) = if release {
        take_connection_logs();
        let release = command.into_unsubscribe(UUID4::new(), UnixNanos::from(37), None);
        adapter.execute_unsubscribe(&release);
        ("Unsubscribe", "unreleased", release.command_id())
    } else {
        ("Subscribe", "unaccepted", command_id)
    };

    let failed = if bars {
        format!("Bars({second_bar:?})")
    } else {
        format!("MarkPrices({second:?})")
    };

    let errors: Vec<_> = take_connection_logs()
        .into_iter()
        .filter(|(level, _)| *level == Level::Error)
        .collect();
    assert_eq!(
        errors,
        vec![(
            Level::Error,
            format!(
                "Client command failed: {direction} {client_id} {outcome} [{failed}] command {operation_id}"
            )
        )]
    );
}

fn capture_connection_logs() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        log::set_logger(&ConnectionLogCapture).expect("test logger already installed");
        log::set_max_level(LevelFilter::Trace);
    });

    CONNECTION_LOGS.with_borrow_mut(Vec::clear);
}

fn take_connection_logs() -> Vec<(Level, String)> {
    CONNECTION_LOGS.with_borrow_mut(std::mem::take)
}

struct ConnectionLogCapture;

impl Log for ConnectionLogCapture {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        matches!(
            metadata.target(),
            "nautilus_data::engine" | "nautilus_common::clients"
        )
    }

    fn log(&self, record: &Record<'_>) {
        if self.enabled(record.metadata()) {
            CONNECTION_LOGS.with_borrow_mut(|messages| {
                messages.push((record.level(), record.args().to_string()));
            });
        }
    }

    fn flush(&self) {}
}

struct ConnectionDataClient {
    client_id: ClientId,
    error_message: Option<&'static str>,
    error_operation: Option<&'static str>,
    barrier: Rc<tokio::sync::Barrier>,
    connected: bool,
    calls: Rc<RefCell<Vec<(ClientId, &'static str)>>>,
}

impl ConnectionDataClient {
    fn record(&self, operation: &'static str) -> anyhow::Result<()> {
        self.calls.borrow_mut().push((self.client_id, operation));

        if let Some(error_message) = self.error_message
            && self
                .error_operation
                .is_none_or(|selected| selected == operation)
        {
            anyhow::bail!("{error_message}");
        }

        Ok(())
    }
}

#[async_trait::async_trait(?Send)]
impl DataClient for ConnectionDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        None
    }

    fn start(&mut self) -> anyhow::Result<()> {
        self.record("start")
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        self.record("stop")
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        self.record("reset")
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        self.record("dispose")
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        self.barrier.wait().await;

        self.record("connect")?;
        self.connected = true;
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        self.barrier.wait().await;
        self.record("disconnect")?;
        self.connected = false;
        Ok(())
    }

    fn subscribe_quotes(&mut self, _: SubscribeQuotes) -> anyhow::Result<()> {
        self.record("subscribe")
    }

    fn unsubscribe_quotes(&mut self, _: &UnsubscribeQuotes) -> anyhow::Result<()> {
        self.record("unsubscribe")
    }

    #[cfg(feature = "defi")]
    fn subscribe_blocks(&mut self, _: SubscribeBlocks) -> anyhow::Result<()> {
        self.record("subscribe")
    }

    #[cfg(feature = "defi")]
    fn unsubscribe_blocks(&mut self, _: &UnsubscribeBlocks) -> anyhow::Result<()> {
        self.record("unsubscribe")
    }

    fn request_quotes(&self, request: RequestQuotes) -> anyhow::Result<()> {
        request
            .single_instrument_id()
            .map_err(|e| e.context("provider context token=SYN-private"))?;
        self.record("request")
    }

    fn is_connected(&self) -> bool {
        self.connected
    }

    fn is_disconnected(&self) -> bool {
        !self.connected
    }
}

#[rstest]
#[case(Some("password=SYN-password bearer=SYN-token"))]
#[case(Some("https://SYN-user:SYN-password@example.invalid/SYN-path?token=SYN-token"))]
#[case(None)]
#[tokio::test]
async fn test_data_client_lifecycle_logging(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    #[case] error_message: Option<&'static str>,
) {
    capture_connection_logs();
    let mut engine = DataEngine::new(clock, cache, None);
    let calls = Rc::new(RefCell::new(Vec::new()));
    let barrier = Rc::new(tokio::sync::Barrier::new(2));

    for (client_id, error) in [
        (ClientId::from("FIRST"), error_message),
        (ClientId::from("SECOND"), None),
    ] {
        let client = ConnectionDataClient {
            client_id,
            error_message: error,
            error_operation: None,
            barrier: Rc::clone(&barrier),
            connected: false,
            calls: Rc::clone(&calls),
        };

        engine.register_client(
            DataClientAdapter::new(client_id, None, true, true, Box::new(client)),
            None,
        );
    }

    take_connection_logs();

    for (operation, action) in [
        ("start", DataEngine::start as fn(&mut DataEngine)),
        ("stop", DataEngine::stop),
        ("reset", DataEngine::reset),
        ("dispose", DataEngine::dispose),
    ] {
        action(&mut engine);

        let expected = if error_message.is_some() {
            vec![(
                Level::Error,
                format!("Failed to {operation} data client FIRST"),
            )]
        } else {
            vec![]
        };

        assert_eq!(take_connection_logs(), expected);
    }

    let result = tokio::time::timeout(std::time::Duration::from_secs(1), engine.disconnect())
        .await
        .expect("disconnect remains concurrent");

    if error_message.is_some() {
        assert_eq!(
            result.unwrap_err().to_string(),
            "Failed to disconnect data clients: FIRST"
        );
    } else {
        result.unwrap();
    }

    let expected_calls: Vec<_> = ["start", "stop", "reset", "dispose"]
        .into_iter()
        .flat_map(|op| {
            [
                (ClientId::from("FIRST"), op),
                (ClientId::from("SECOND"), op),
            ]
        })
        .collect();

    let actual_calls = calls.borrow();
    assert_eq!(actual_calls[..8], expected_calls);
    let mut disconnect_calls = actual_calls[8..].to_vec();
    disconnect_calls.sort_by_key(|(client_id, _)| *client_id);
    assert_eq!(
        disconnect_calls,
        vec![
            (ClientId::from("FIRST"), "disconnect"),
            (ClientId::from("SECOND"), "disconnect")
        ]
    );
    assert_eq!(take_connection_logs(), vec![]);
}

#[rstest]
#[case(Some("password=SYN-password token=SYN-token"))]
#[case(None)]
fn test_data_subscription_logging(
    #[case] error_message: Option<&'static str>,
    #[values("subscribe", "unsubscribe")] operation: &'static str,
) {
    capture_connection_logs();
    let client_id = ClientId::from("SUBSCRIPTIONS");
    let calls = Rc::new(RefCell::new(Vec::new()));

    let client = ConnectionDataClient {
        client_id,
        error_message,
        error_operation: Some(operation),
        barrier: Rc::new(tokio::sync::Barrier::new(1)),
        connected: false,
        calls: Rc::clone(&calls),
    };

    let mut adapter = DataClientAdapter::new(client_id, None, true, true, Box::new(client));
    let instrument_id = InstrumentId::from("SYN.SYN");
    let params =
        Params::new([("token".to_string(), serde_json::json!("SYN-command-secret"))].into());
    let subscribe_id = UUID4::new();
    let unsubscribe_id = UUID4::new();
    adapter.execute_subscribe(SubscribeCommand::Quotes(SubscribeQuotes::new(
        instrument_id,
        Some(client_id),
        None,
        subscribe_id,
        UnixNanos::from(1),
        None,
        Some(params.clone()),
    )));
    adapter.execute_unsubscribe(&UnsubscribeCommand::Quotes(UnsubscribeQuotes::new(
        instrument_id,
        Some(client_id),
        None,
        unsubscribe_id,
        UnixNanos::from(2),
        None,
        Some(params),
    )));

    let expected = if error_message.is_some() {
        let (name, outcome, command_id) = if operation == "subscribe" {
            ("Subscribe", "unaccepted", subscribe_id)
        } else {
            ("Unsubscribe", "unreleased", unsubscribe_id)
        };

        vec![(
            Level::Error,
            format!(
                "Client command failed: {name} SUBSCRIPTIONS {outcome} [Quotes(\"SYN.SYN\")] command {command_id}"
            ),
        )]
    } else {
        vec![]
    };

    assert_eq!(take_connection_logs(), expected);

    let expected_calls = if error_message.is_some() && operation == "subscribe" {
        vec![(client_id, "subscribe")]
    } else {
        vec![(client_id, "subscribe"), (client_id, "unsubscribe")]
    };

    assert_eq!(*calls.borrow(), expected_calls);
    assert_eq!(
        adapter.subscriptions_quotes.contains(&instrument_id),
        error_message.is_some() && operation == "unsubscribe"
    );
}

#[cfg(feature = "defi")]
#[rstest]
#[case(Some("password=SYN-password token=SYN-token"))]
#[case(None)]
fn test_defi_subscription_logging(
    #[case] error_message: Option<&'static str>,
    #[values("subscribe", "unsubscribe")] operation: &'static str,
) {
    use nautilus_common::messages::defi::{
        DefiSubscribeCommand, DefiUnsubscribeCommand, SubscribeBlocks, UnsubscribeBlocks,
    };
    use nautilus_model::defi::Blockchain;
    capture_connection_logs();
    let client_id = ClientId::from("DEFI");
    let calls = Rc::new(RefCell::new(Vec::new()));

    let client = ConnectionDataClient {
        client_id,
        error_message,
        error_operation: Some(operation),
        barrier: Rc::new(tokio::sync::Barrier::new(1)),
        connected: false,
        calls: Rc::clone(&calls),
    };

    let mut adapter = DataClientAdapter::new(client_id, None, true, true, Box::new(client));
    let params =
        Params::new([("token".to_string(), serde_json::json!("SYN-command-secret"))].into());
    let subscribe_id = UUID4::new();
    let unsubscribe_id = UUID4::new();
    adapter.execute_defi_subscribe(DefiSubscribeCommand::Blocks(SubscribeBlocks::new(
        Blockchain::Ethereum,
        Some(client_id),
        subscribe_id,
        UnixNanos::from(1),
        Some(params.clone()),
    )));
    adapter.execute_defi_unsubscribe(&DefiUnsubscribeCommand::Blocks(UnsubscribeBlocks::new(
        Blockchain::Ethereum,
        Some(client_id),
        unsubscribe_id,
        UnixNanos::from(2),
        Some(params),
    )));

    let expected = if error_message.is_some() {
        let (name, command_id) = if operation == "subscribe" {
            ("DefiSubscribe", subscribe_id)
        } else {
            ("DefiUnsubscribe", unsubscribe_id)
        };

        vec![(
            Level::Error,
            format!("Client command failed: {name} DEFI Blocks(Ethereum) command {command_id}"),
        )]
    } else {
        vec![]
    };

    assert_eq!(take_connection_logs(), expected);

    let expected_calls = if error_message.is_some() && operation == "subscribe" {
        vec![(client_id, "subscribe")]
    } else {
        vec![(client_id, "subscribe"), (client_id, "unsubscribe")]
    };

    assert_eq!(*calls.borrow(), expected_calls);
    assert_eq!(
        adapter.subscriptions_blocks.contains(&Blockchain::Ethereum),
        error_message.is_some() && operation == "unsubscribe"
    );
}

#[rstest]
#[case(Some("password=SYN-password token=SYN-token"))]
#[case(None)]
fn test_data_request_logging(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
    #[case] error_message: Option<&'static str>,
) {
    capture_connection_logs();
    let client_id = ClientId::from("REQUESTS");
    let request_id = UUID4::new();
    let calls = Rc::new(RefCell::new(Vec::new()));

    let client = ConnectionDataClient {
        client_id,
        error_message,
        error_operation: None,
        barrier: Rc::new(tokio::sync::Barrier::new(1)),
        connected: false,
        calls: Rc::clone(&calls),
    };

    let mut engine = DataEngine::new(clock, cache, None);
    engine.register_client(
        DataClientAdapter::new(client_id, None, true, true, Box::new(client)),
        None,
    );
    take_connection_logs();

    let request = RequestQuotes {
        instrument_ids: vec![InstrumentId::from("SYN.SYN")],
        start: None,
        end: None,
        limit: None,
        client_id: Some(client_id),
        request_id,
        ts_init: UnixNanos::from(1),
        params: None,
    };

    let (handler, saver) = get_typed_message_saving_handler::<DataResponseEnvelope>(None);
    msgbus::register_response_handler(&request_id, handler);
    engine.execute(DataCommand::Request(RequestCommand::Quotes(request)));

    let expected = if error_message.is_some() {
        vec![(
            Level::Error,
            format!(
                "Data command failed: (\"Request\", Some(\"REQUESTS\"), Some(\"SYN\"), Some({request_id:?})): Data client REQUESTS request failed"
            ),
        )]
    } else {
        vec![]
    };

    assert_eq!(take_connection_logs(), expected);
    assert_eq!(*calls.borrow(), vec![(client_id, "request")]);
    let responses = saver.get_messages();
    assert_eq!(responses.len(), usize::from(error_message.is_some()));

    if error_message.is_some() {
        assert_eq!(responses[0].correlation_id(), &request_id);
        assert!(responses[0].completed());
        assert!(responses[0].response().is_none());
        assert_eq!(
            responses[0].failure_reason(),
            Some("Data client REQUESTS request failed")
        );
    }
}

#[cfg(feature = "streaming")]
#[rstest]
#[case::unsupported(true)]
#[case::operational(false)]
fn test_data_request_failure_preserves_only_safe_reason(
    #[case] unsupported: bool,
    #[values(false, true)] planned: bool,
    cache: Rc<RefCell<Cache>>,
    stub_msgbus: Rc<RefCell<MessageBus>>,
) {
    let client_id = ClientId::from("REQUESTS");
    let request_id = UUID4::new();
    let calls = Rc::new(RefCell::new(Vec::new()));

    let client = ConnectionDataClient {
        client_id,
        error_message: Some("provider password=SYN-private"),
        error_operation: None,
        barrier: Rc::new(tokio::sync::Barrier::new(1)),
        connected: false,
        calls: Rc::clone(&calls),
    };

    let mut engine = DataEngine::new(data_engine_clock_at(3000), cache, None);
    engine.register_client(
        DataClientAdapter::new(client_id, None, true, true, Box::new(client)),
        None,
    );
    let directory = planned.then(|| CatalogTempDir::new("request-rejection"));
    if let Some(directory) = &directory {
        engine.register_catalog(
            Box::new(ParquetDataCatalog::new(
                directory.path(),
                None,
                None,
                None,
                None,
            )),
            Some("history"),
        );
    }

    let mut ids = vec![InstrumentId::from("AUD/USD.SIM")];
    if unsupported {
        ids.push(InstrumentId::from("GBP/USD.SIM"));
    }

    let request = RequestQuotes::new(
        ids,
        Some(UnixNanos::from(1000).to_datetime_utc()),
        Some(UnixNanos::from(2000).to_datetime_utc()),
        None,
        Some(client_id),
        request_id,
        UnixNanos::from(3000),
        None,
    );
    let (handler, saver) = get_typed_message_saving_handler::<DataResponseEnvelope>(None);
    msgbus::register_response_handler(&request_id, handler);
    let result = engine.execute_request(RequestCommand::Quotes(request));
    let replies = saver.get_messages();

    let expected_reason = if unsupported {
        "Data client REQUESTS request failed: request requires one instrument ID on this adapter path, received 2"
    } else {
        "Data client REQUESTS request failed"
    };

    let expected_calls = if unsupported {
        Vec::new()
    } else {
        vec![(client_id, "request")]
    };

    assert_eq!(
        result.err().map(|e| e.to_string()),
        (!planned).then(|| expected_reason.to_owned())
    );
    assert_eq!(*calls.borrow(), expected_calls);
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0].correlation_id(), &request_id);
    assert_eq!(replies[0].sub_correlation_id(), None);
    assert!(replies[0].completed());
    assert!(replies[0].response().is_none());
    assert_eq!(replies[0].failure_reason(), Some(expected_reason));
    assert_eq!(engine.request_pipeline_count(), 0);
    assert!(
        stub_msgbus
            .borrow()
            .get_response_handler(&request_id)
            .is_none()
    );
}

#[rstest]
fn test_data_request_logging_preserves_venue_without_client(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    capture_connection_logs();
    let request_id = UUID4::new();

    let request = RequestQuotes {
        instrument_ids: vec![InstrumentId::from("BTCUSDT.BINANCE")],
        start: None,
        end: None,
        limit: None,
        client_id: None,
        request_id,
        ts_init: UnixNanos::from(1),
        params: None,
    };

    let mut engine = DataEngine::new(clock, cache, None);
    take_connection_logs();
    engine.execute(DataCommand::Request(RequestCommand::Quotes(request)));
    assert_eq!(
        take_connection_logs(),
        vec![(
            Level::Error,
            format!(
                "Data command failed: (\"Request\", None, Some(\"BINANCE\"), Some({request_id:?})): Cannot handle request: no client found for None Some(\"BINANCE\")"
            )
        )]
    );
}

#[derive(Debug)]
struct UnsupportedDataClient;

#[async_trait::async_trait(?Send)]
impl DataClient for UnsupportedDataClient {
    fn client_id(&self) -> ClientId {
        ClientId::from("UNSUPPORTED")
    }

    fn venue(&self) -> Option<Venue> {
        None
    }

    fn start(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn is_connected(&self) -> bool {
        false
    }

    fn is_disconnected(&self) -> bool {
        true
    }
}

#[rstest]
fn test_unimplemented_command_logging_omits_parameters() {
    capture_connection_logs();

    let params = nautilus_core::Params::new(
        [("api_key".to_string(), serde_json::json!("SYN-secret"))].into(),
    );

    let request = RequestQuotes {
        instrument_ids: vec![InstrumentId::from("SYN.SYN")],
        start: None,
        end: None,
        limit: None,
        client_id: None,
        request_id: UUID4::new(),
        ts_init: UnixNanos::from(1),
        params: Some(params.clone()),
    };

    let unsubscribe = UnsubscribeQuotes::new(
        InstrumentId::from("SYN.SYN"),
        Some(ClientId::from("UNSUPPORTED")),
        None,
        UUID4::new(),
        UnixNanos::from(2),
        None,
        Some(params),
    );
    let mut client = UnsupportedDataClient;
    let result = client.request_quotes(request);
    let unsubscribe_result = client.unsubscribe_quotes(&unsubscribe);

    assert_eq!(
        result.unwrap_err().to_string(),
        "Data client UNSUPPORTED does not support RequestQuotes"
    );
    assert!(unsubscribe_result.is_ok());
    assert_eq!(
        take_connection_logs(),
        vec![(
            Level::Warn,
            format!(
                "Handler not implemented: client UNSUPPORTED, command {}",
                std::any::type_name::<UnsubscribeQuotes>()
            )
        )]
    );
}

#[rstest]
fn test_engine_preparation_logs_each_failure_reason(
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    capture_connection_logs();
    let command_id = UUID4::new();
    let bar_types = [
        BarType::from("FIRST.SIM-1-MINUTE-LAST-INTERNAL"),
        BarType::from("SECOND.SIM-5-MINUTE-LAST-INTERNAL"),
    ];

    let command = SubscribeBars::new(
        bar_types,
        Some(ClientId::from("PUBLIC")),
        Some(Venue::from("SIM")),
        command_id,
        UnixNanos::from(17),
        None,
        Some(params_from_json(json!({"private": "must-not-log"}))),
    );
    let mut engine = DataEngine::new(clock, cache, None);
    take_connection_logs();

    let error = engine
        .execute_subscribe(SubscribeCommand::Bars(command))
        .unwrap_err();

    let expected: Vec<_> = bar_types
        .into_iter()
        .map(|bar_type| {
            (
                Level::Error,
                format!(
                    "Subscribe preparation failed: command={command_id}, client=Some(\"PUBLIC\"), instrument_ids=None, bar_types=Some([{bar_type:?}]): Cannot start bar aggregation: no instrument found for {}",
                    bar_type.instrument_id(),
                ),
            )
        })
        .collect();

    assert_eq!(
        error.to_string(),
        "Cannot start bar aggregation: no instrument found for FIRST.SIM"
    );
    assert_eq!(take_connection_logs(), expected);
}

#[rstest]
fn test_engine_validation_logging_preserves_reason(
    #[values(false, true)] subscribe: bool,
    clock: Rc<RefCell<VirtualClock>>,
    cache: Rc<RefCell<Cache>>,
) {
    capture_connection_logs();
    let command_id = UUID4::new();

    let command = SubscribeInstrument {
        instrument_ids: vec![InstrumentId::from("SYNTH.SYNTH")],
        client_id: Some(ClientId::from("PUBLIC")),
        venue: Some(Venue::from("SYNTH")),
        command_id,
        ts_init: UnixNanos::from(1),
        correlation_id: None,
        params: None,
    };

    let mut engine = DataEngine::new(clock, cache, None);
    take_connection_logs();
    let command = SubscribeCommand::Instrument(command);

    let (action, reason, command) = if subscribe {
        (
            "Subscribe",
            "Cannot subscribe for synthetic instrument `Instrument` data",
            DataCommand::Subscribe(command),
        )
    } else {
        (
            "Unsubscribe",
            "Cannot unsubscribe from synthetic instrument `Instrument` data",
            DataCommand::Unsubscribe(command.into_unsubscribe(
                command_id,
                UnixNanos::from(1),
                None,
            )),
        )
    };

    engine.execute(command);
    assert_eq!(
        take_connection_logs(),
        vec![
            (
                Level::Error,
                format!(
                    "{action} preparation failed: command={command_id}, client=Some(\"PUBLIC\"), instrument_ids=Some([\"SYNTH.SYNTH\"]), bar_types=None: {reason}"
                )
            ),
            (
                Level::Error,
                format!(
                    "Data command failed: (\"{action}\", Some(\"PUBLIC\"), Some(\"SYNTH\"), Some({command_id:?})): {reason}"
                )
            )
        ]
    );
}

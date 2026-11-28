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

use futures_util::poll;
use indexmap::IndexMap;
use nautilus_common::{cache::Cache, live::dst::time};
use nautilus_testkit::{cache::TestCacheDatabaseControl, components::StateStrategy};

use super::*;

const STOP_CHECK_WINDOW: Duration = Duration::from_secs(1);
const HALF_GRACE: Duration = Duration::from_secs(1);
const SHUTDOWN_DELIVERIES: usize = 4;
const SHUTDOWN_TRANSITIONS: [ShutdownAction; 4] = [
    ShutdownAction::Stop,
    ShutdownAction::StopAgain,
    ShutdownAction::EndGrace,
    ShutdownAction::Finish,
];
const SHUTDOWN_DEPTH: usize = SHUTDOWN_DELIVERIES + SHUTDOWN_TRANSITIONS.len();
const STRATEGY_ON_SAVE: &str = "strategy.on_save";

#[derive(Clone, Copy)]
enum StartupStop {
    PendingReport,
    ReadyReport,
    Running,
}

#[derive(Debug, PartialEq, Eq)]
enum StartupCallback {
    Start(Decimal),
    Stop,
}

#[derive(Debug)]
struct StartupProbe {
    core: StrategyCore,
    callbacks: Rc<RefCell<Vec<StartupCallback>>>,
}

impl DataActor for StartupProbe {
    fn on_start(&mut self) -> anyhow::Result<()> {
        let quantity = self
            .cache()
            .positions_open(None, None, None, None, None)
            .iter()
            .map(|position| position.signed_decimal_qty())
            .sum();
        self.callbacks
            .borrow_mut()
            .push(StartupCallback::Start(quantity));
        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        self.callbacks.borrow_mut().push(StartupCallback::Stop);
        Ok(())
    }
}

nautilus_strategy!(StartupProbe);

mod dst {
    use super::*;

    #[rstest]
    #[case::after_recovery(StartupStop::Running, StartupMassStatusBehavior::Available, None)]
    #[case::before_reply(StartupStop::PendingReport, StartupMassStatusBehavior::Available, None)]
    #[case::with_ready_reply(StartupStop::ReadyReport, StartupMassStatusBehavior::Available, None)]
    #[case::before_error(
        StartupStop::PendingReport,
        StartupMassStatusBehavior::Error,
        Some("Failed to get mass status from LIFECYCLE-REPORT")
    )]
    #[case::with_ready_error(
        StartupStop::ReadyReport,
        StartupMassStatusBehavior::Error,
        Some("Failed to get mass status from LIFECYCLE-REPORT")
    )]
    #[case::before_timeout(
        StartupStop::PendingReport,
        StartupMassStatusBehavior::Pending,
        Some(
            "Startup reconciliation timeout reached while requesting mass status from LIFECYCLE-REPORT"
        )
    )]
    #[cfg_attr(
        not(all(feature = "simulation", madsim)),
        tokio::test(start_paused = true)
    )]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_startup_reconciliation_stop_schedules(
        #[case] stop: StartupStop,
        #[case] behavior: StartupMassStatusBehavior,
        #[case] expected_error: Option<&str>,
    ) {
        let (release, gate) = tokio::sync::oneshot::channel();

        let state = StartupMassStatusClientState {
            mass_status_gate: Arc::new(Mutex::new(Some(gate))),
            ..Default::default()
        };

        let (mut node, quantity) = startup_node(&state, behavior);
        let callbacks = Rc::new(RefCell::new(Vec::new()));
        node.add_strategy(StartupProbe {
            core: StrategyCore::new(StrategyConfig::default()),
            callbacks: callbacks.clone(),
        })
        .unwrap();

        let handle = node.handle();

        let result = {
            let run = node.run_with_mode(NodeRunMode::Hosted);
            tokio::pin!(run);
            assert!(poll!(run.as_mut()).is_pending());
            assert_eq!(handle.state(), NodeState::Starting);
            assert!(state.mass_status_requested.load(Ordering::Relaxed));
            assert!(state.connected.load(Ordering::Relaxed));
            assert!(callbacks.borrow().is_empty());

            if matches!(stop, StartupStop::PendingReport) {
                handle.stop();
                assert!(poll!(run.as_mut()).is_pending());
                assert_eq!(handle.state(), NodeState::Starting);
            }

            release.send(()).unwrap();

            if matches!(stop, StartupStop::Running) {
                assert!(poll!(run.as_mut()).is_pending());
                assert_eq!(handle.state(), NodeState::Running);
                assert_eq!(*callbacks.borrow(), [StartupCallback::Start(quantity)]);
            }

            if matches!(stop, StartupStop::ReadyReport | StartupStop::Running) {
                handle.stop();
            }

            time::timeout(Duration::from_secs(5), run)
                .await
                .expect("node did not finish the startup schedule")
        };

        assert_eq!(
            result.as_ref().err().map(ToString::to_string).as_deref(),
            expected_error
        );

        if matches!(behavior, StartupMassStatusBehavior::Error) {
            assert_eq!(
                result.unwrap_err().root_cause().to_string(),
                "mass status failed"
            );
        }

        let expected_callbacks = if matches!(stop, StartupStop::Running) {
            vec![StartupCallback::Start(quantity), StartupCallback::Stop]
        } else {
            vec![]
        };

        let expected_positions = if matches!(behavior, StartupMassStatusBehavior::Available) {
            vec![quantity]
        } else {
            vec![]
        };

        let positions: Vec<_> = node
            .kernel()
            .cache()
            .borrow()
            .positions_open(None, None, None, None, None)
            .iter()
            .map(|position| position.signed_decimal_qty())
            .collect();

        assert_eq!(*callbacks.borrow(), expected_callbacks);
        assert_eq!(positions, expected_positions);
        assert_eq!(handle.state(), NodeState::Stopped);
        assert!(state.disconnect_attempted.load(Ordering::Relaxed));
        assert!(!state.connected.load(Ordering::Relaxed));
        assert_eq!(state.stops.load(Ordering::Relaxed), 1);
        node.dispose();
    }

    #[rstest]
    #[case::clean(false, false)]
    #[case::disconnect_timeout(true, false)]
    #[case::save_error(false, true)]
    #[case::combined_errors(true, true)]
    #[cfg_attr(
        not(all(feature = "simulation", madsim)),
        tokio::test(start_paused = true)
    )]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_shutdown_schedules_drain_events_and_finalize(
        #[case] disconnect_timeout: bool,
        #[case] save_error: bool,
    ) {
        for schedule in shutdown_schedules() {
            check_shutdown_schedule(schedule, disconnect_timeout, save_error).await;
        }
    }

    async fn check_shutdown_schedule(
        schedule: [ShutdownAction; SHUTDOWN_DEPTH],
        disconnect_timeout: bool,
        save_error: bool,
    ) {
        let (release, gate) = tokio::sync::oneshot::channel();

        let data = LifecycleClientState {
            disconnect_gate: Arc::new(Mutex::new(Some(gate))),
            ..Default::default()
        };

        let exec = LifecycleClientState::default();
        let (mut node, control) = shutdown_node(&data, &exec, save_error);
        let handle = node.handle();
        let cache = node.kernel().cache();
        let updates = seed_shutdown_orders(&mut cache.borrow_mut());
        let mut stage = ShutdownStage::Running;
        let mut applied = 0;
        let topic = switchboard::get_event_order_topic(updates[0].strategy_id());
        let published = Rc::new(RefCell::new(Vec::new()));

        let handler = TypedHandler::from({
            let control = control.clone();
            let published = published.clone();
            move |event: &OrderEventAny| {
                let saved = control
                    .events()
                    .iter()
                    .any(|callback| callback == STRATEGY_ON_SAVE);
                published.borrow_mut().push((event.clone(), saved));
            }
        });

        msgbus::subscribe_order_events(topic.into(), handler.clone(), None);

        let result = {
            let run = node.run_with_mode(NodeRunMode::Hosted);
            tokio::pin!(run);
            assert!(poll!(run.as_mut()).is_pending(), "{schedule:?}");
            assert_eq!(handle.state(), NodeState::Running, "{schedule:?}");
            assert!(data.connected.load(Ordering::Relaxed), "{schedule:?}");
            assert!(exec.connected.load(Ordering::Relaxed), "{schedule:?}");
            let sender = get_exec_event_sender();

            for action in schedule {
                match action {
                    ShutdownAction::Stop => {
                        handle.stop();
                        advance_clock(STOP_CHECK_WINDOW).await;
                        stage = ShutdownStage::Grace;
                    }
                    ShutdownAction::StopAgain => {
                        advance_clock(HALF_GRACE).await;
                        assert!(poll!(run.as_mut()).is_pending(), "{schedule:?}");
                        handle.stop();
                    }
                    ShutdownAction::EndGrace => {
                        advance_clock(HALF_GRACE).await;
                        stage = ShutdownStage::Disconnecting;
                    }
                    ShutdownAction::Deliver(index) => {
                        sender
                            .send(ExecutionEvent::Order(updates[index].clone()))
                            .unwrap();

                        if stage != ShutdownStage::Disconnecting {
                            applied = index + 1;
                        }
                    }
                    ShutdownAction::Finish => break,
                }

                assert!(
                    poll!(run.as_mut()).is_pending(),
                    "{schedule:?} at {action:?}"
                );
                stage.assert_observed(&handle, &data, &exec, &control, &schedule, action);
                assert_order_updates(&cache.borrow(), &updates, applied, &schedule);
            }

            if !disconnect_timeout {
                release.send(()).unwrap();
            }

            time::timeout(Duration::from_secs(5), run)
                .await
                .unwrap_or_else(|_| panic!("node did not finish {schedule:?}"))
        };

        msgbus::unsubscribe_order_events(topic.into(), &handler);
        assert_shutdown_result(result, disconnect_timeout, save_error, &schedule);
        assert_eq!(handle.state(), NodeState::Stopped, "{schedule:?}");
        assert_eq!(
            data.connected.load(Ordering::Relaxed),
            disconnect_timeout,
            "{schedule:?}"
        );
        assert_eq!(data.stops.load(Ordering::Relaxed), 1, "{schedule:?}");
        assert_eq!(exec.stops.load(Ordering::Relaxed), 1, "{schedule:?}");
        assert_order_updates(&cache.borrow(), &updates, updates.len(), &schedule);
        let expected_publications: Vec<_> = updates
            .into_iter()
            .enumerate()
            .map(|(index, event)| (event, index >= applied))
            .collect();
        assert_eq!(*published.borrow(), expected_publications, "{schedule:?}");

        let mut events = vec!["strategy.on_start", "strategy.on_stop", STRATEGY_ON_SAVE];
        if !save_error {
            events.push("strategy.update:LIFECYCLE-001");
        }

        assert_eq!(control.events(), events, "{schedule:?}");

        node.dispose();
        events.push("database.close");
        assert_eq!(control.events(), events, "{schedule:?}");
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    async fn advance_clock(duration: Duration) {
        tokio::time::advance(duration).await;
        tokio::task::yield_now().await;
    }

    #[cfg(all(feature = "simulation", madsim))]
    async fn advance_clock(duration: Duration) {
        madsim::time::advance(duration);
        madsim::task::yield_now().await;
    }
}

fn startup_node(
    state: &StartupMassStatusClientState,
    behavior: StartupMassStatusBehavior,
) -> (LiveNode, Decimal) {
    let instrument = crypto_perpetual_ethusdt();
    let venue = instrument.id().venue;
    let client_id = ClientId::from("LIFECYCLE-REPORT");
    let account_id = AccountId::from("BLOCKING-REPORT-001");
    let quantity = Quantity::from("5.125");
    let mut mass_status =
        ExecutionMassStatus::new(client_id, account_id, venue, UnixNanos::default(), None);
    mass_status.add_position_reports(vec![reconciliation_position_report(quantity)]);
    *state.mass_status.lock() = Some(mass_status);

    let mut config = lifecycle_config();
    config.exec_engine.reconciliation = true;
    config.exec_engine.generate_missing_orders = true;
    let factory = StartupMassStatusExecutionClientFactory::new(state.clone(), behavior)
        .with_identity(client_id, account_id, venue)
        .with_oms_type(OmsType::Netting);
    let node = LiveNodeBuilder::from_config(config)
        .unwrap()
        .with_name("StartupSchedule")
        .add_exec_client(
            None,
            Box::new(factory),
            Box::new(StartupMassStatusExecutionClientConfig),
        )
        .unwrap()
        .build()
        .unwrap();
    node.kernel()
        .cache()
        .borrow_mut()
        .add_instrument(InstrumentAny::CryptoPerpetual(instrument))
        .unwrap();
    add_reconciliation_test_account(&node);
    (node, quantity.as_decimal())
}

#[derive(Clone, Copy, Debug)]
enum ShutdownAction {
    Stop,
    StopAgain,
    EndGrace,
    Deliver(usize),
    Finish,
}

fn shutdown_schedules() -> impl Iterator<Item = [ShutdownAction; SHUTDOWN_DEPTH]> {
    // Interleave four ordered events with three shutdown transitions, then finish disconnect
    (0_usize..1 << (SHUTDOWN_DEPTH - 1))
        .filter(|mask| mask.count_ones() as usize == SHUTDOWN_DELIVERIES)
        .map(|mask| {
            let mut deliveries = (0..SHUTDOWN_DELIVERIES).map(ShutdownAction::Deliver);
            let mut transitions = SHUTDOWN_TRANSITIONS.into_iter();
            std::array::from_fn(|index| {
                if mask & (1 << index) != 0 {
                    deliveries.next().unwrap()
                } else {
                    transitions.next().unwrap()
                }
            })
        })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ShutdownStage {
    Running,
    Grace,
    Disconnecting,
}

impl ShutdownStage {
    fn assert_observed(
        self,
        handle: &LiveNodeHandle,
        data: &LifecycleClientState,
        exec: &LifecycleClientState,
        control: &TestCacheDatabaseControl,
        schedule: &[ShutdownAction],
        action: ShutdownAction,
    ) {
        let (state, callbacks) = if self == Self::Running {
            (NodeState::Running, &["strategy.on_start"][..])
        } else {
            (
                NodeState::ShuttingDown,
                &["strategy.on_start", "strategy.on_stop"][..],
            )
        };

        let disconnecting = self == Self::Disconnecting;
        assert_eq!(handle.state(), state, "{schedule:?} at {action:?}");
        assert_eq!(control.events(), callbacks, "{schedule:?} at {action:?}");
        assert_eq!(
            (
                data.stops.load(Ordering::Relaxed),
                exec.stops.load(Ordering::Relaxed),
            ),
            (0, 0),
            "{schedule:?} at {action:?}: data and execution stop counts",
        );
        assert_eq!(
            (
                data.disconnect_attempted.load(Ordering::Relaxed),
                exec.disconnect_attempted.load(Ordering::Relaxed),
                exec.connected.load(Ordering::Relaxed),
            ),
            (disconnecting, disconnecting, !disconnecting),
            "{schedule:?} at {action:?}: data and execution disconnect attempts, execution connected",
        );
    }
}

fn shutdown_node(
    data: &LifecycleClientState,
    exec: &LifecycleClientState,
    save_error: bool,
) -> (LiveNode, TestCacheDatabaseControl) {
    let mut config = lifecycle_config();
    config.save_state = true;
    config.delay_post_stop = HALF_GRACE * 2;
    let mut node = LiveNodeBuilder::from_config(config)
        .unwrap()
        .with_name("ShutdownSchedule")
        .add_data_client(
            None,
            Box::new(LifecycleDataClientFactory::new(
                data.clone(),
                LifecycleClientBehavior::Connects,
            )),
            Box::new(LifecycleDataClientConfig),
        )
        .unwrap()
        .add_exec_client(
            None,
            Box::new(LifecycleExecutionClientFactory::new(
                exec.clone(),
                LifecycleClientBehavior::Connects,
            )),
            Box::new(LifecycleExecutionClientConfig),
        )
        .unwrap()
        .build()
        .unwrap();
    let (database, control) = TestCacheDatabaseControl::create();
    node.set_cache_database(Box::new(database)).unwrap();

    let strategy = StateStrategy::new(
        StrategyId::from("LIFECYCLE-001"),
        control.clone(),
        IndexMap::new(),
    );

    let strategy = if save_error {
        strategy.with_fail_save()
    } else {
        strategy
    };

    node.add_strategy(strategy).unwrap();
    (node, control)
}

fn seed_shutdown_orders(cache: &mut Cache) -> [OrderEventAny; SHUTDOWN_DELIVERIES] {
    std::array::from_fn(|index| {
        let order = OrderTestBuilder::new(OrderType::Market)
            .client_order_id(ClientOrderId::from(format!("O-SHUTDOWN-{index}")))
            .instrument_id(InstrumentId::from("EUR/USD.SIM"))
            .quantity(Quantity::from("2"))
            .build();
        let submitted = TestOrderEventStubs::submitted(&order, AccountId::from("SIM-001"));
        cache.add_order(order, None, None, false).unwrap();
        submitted
    })
}

fn assert_order_updates(
    cache: &Cache,
    updates: &[OrderEventAny],
    applied: usize,
    schedule: &[ShutdownAction],
) {
    for (index, event) in updates.iter().enumerate() {
        let order = cache.order(&event.client_order_id()).unwrap();
        let submitted = [event];
        let expected = if index < applied {
            (OrderStatus::Submitted, submitted.as_slice())
        } else {
            (OrderStatus::Initialized, &[][..])
        };

        assert_eq!(
            (order.status(), &order.events()[1..]),
            expected,
            "{schedule:?} at delivery {index}",
        );
    }
}

fn assert_shutdown_result(
    result: anyhow::Result<()>,
    disconnect_timeout: bool,
    save_error: bool,
    schedule: &[ShutdownAction],
) {
    let mut errors = Vec::new();
    if disconnect_timeout {
        errors.extend([
            "disconnect timeout while disconnecting clients",
            "failed while awaiting engine disconnection: disconnect readiness timeout while waiting for engine disconnections",
        ]);
    }

    if save_error {
        errors.push("failed while finalizing kernel shutdown: Failed to save component state: strategy LIFECYCLE-001 callback: test strategy on_save failure");
    }

    let actual = result.err().map(|e| e.to_string());
    let expected = (!errors.is_empty()).then(|| errors.join("; "));
    assert_eq!(actual, expected, "{schedule:?}");
}

fn lifecycle_config() -> LiveNodeConfig {
    LiveNodeConfig {
        exec_engine: LiveExecutionEngineConfig {
            reconciliation: false,
            load_cache: false,
            inflight_check_interval_ms: 0,
            ..Default::default()
        },
        timeout_connection: Duration::ZERO,
        timeout_reconciliation: Duration::from_secs(1),
        timeout_portfolio: Duration::ZERO,
        timeout_disconnection: Duration::from_secs(1),
        delay_post_stop: Duration::ZERO,
        logging: LoggerConfig {
            bypass_logging: true,
            ..Default::default()
        },
        ..Default::default()
    }
}

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

use indexmap::IndexMap;
use nautilus_common::msgbus::{TypedHandler, TypedIntoHandler};
use nautilus_data::DataClientAdapter;
use nautilus_model::events::order::spec::OrderSubmittedSpec;
use nautilus_testkit::{cache::TestCacheDatabaseControl, components::StateStrategy};

use super::*;

#[rstest]
#[case::execution_start(None, false)]
#[case::execution_owned(Some(NodeRunMode::Owned), false)]
#[case::execution_hosted(Some(NodeRunMode::Hosted), false)]
#[case::data_start(None, true)]
#[case::data_owned(Some(NodeRunMode::Owned), true)]
#[case::data_hosted(Some(NodeRunMode::Hosted), true)]
#[tokio::test]
async fn test_client_connect_failure_aborts_startup(
    #[case] mode: Option<NodeRunMode>,
    #[case] data: bool,
    #[values(false, true)] pending: bool,
) {
    let config = LiveNodeConfig {
        exec_engine: LiveExecutionEngineConfig {
            reconciliation: false,
            ..Default::default()
        },
        timeout_connection: if pending {
            Duration::from_millis(100)
        } else {
            Duration::from_secs(30)
        },
        timeout_disconnection: Duration::from_millis(100),
        delay_post_stop: Duration::ZERO,
        ..Default::default()
    };

    let mut node = LiveNode::build("FailedConnectNode".to_string(), Some(config)).unwrap();
    let (_, control) = TestCacheDatabaseControl::create();
    node.add_strategy(StateStrategy::new(
        StrategyId::from("CONNECT-001"),
        control.clone(),
        IndexMap::new(),
    ))
    .unwrap();
    let connect_error = "password=SYN-password token=SYN-token";

    let behavior = if pending {
        LifecycleClientBehavior::ConnectPending
    } else {
        LifecycleClientBehavior::ConnectFails(connect_error)
    };

    let state = LifecycleClientState::default();

    if data {
        node.kernel().data_engine.borrow_mut().register_client(
            DataClientAdapter::new(
                ClientId::from("LIFECYCLE-DATA"),
                None,
                true,
                true,
                Box::new(LifecycleDataClient {
                    state: state.clone(),
                    behavior,
                    stop_on_connect: None,
                }),
            ),
            None,
        );
    } else {
        node.kernel()
            .exec_engine
            .borrow_mut()
            .register_client(Box::new(LifecycleExecutionClient {
                state: state.clone(),
                behavior,
            }))
            .unwrap();
    }

    let e = tokio::time::timeout(Duration::from_secs(2), async {
        match mode {
            None => node.start().await,
            Some(mode) => node.run_with_mode(mode).await,
        }
    })
    .await
    .expect("failed connection waited for startup readiness")
    .unwrap_err();

    assert_eq!(
        e.to_string(),
        match (data, pending) {
            (true, true) => "data-connect timeout",
            (true, false) => "Failed to connect data clients: LIFECYCLE-DATA",
            (false, true) => "exec-connect timeout",
            (false, false) => "Failed to connect execution clients: LIFECYCLE-EXEC",
        }
    );
    assert_eq!(e.chain().count(), 1);
    assert_eq!(node.state(), NodeState::Stopped);
    assert_eq!(control.events(), Vec::<String>::new());
    assert!(state.disconnect_attempted.load(Ordering::Relaxed));

    if data {
        assert!(
            node.kernel()
                .data_engine
                .borrow_mut()
                .get_client(Some(&ClientId::from("LIFECYCLE-DATA")), None)
                .unwrap()
                .is_disconnected()
        );
    } else {
        assert!(
            !node
                .kernel()
                .exec_engine
                .borrow()
                .get_client(&ClientId::from("LIFECYCLE-EXEC"))
                .unwrap()
                .is_connected()
        );
    }
}

#[tokio::test]
async fn test_run_drains_execution_events_from_cancelled_start_on_ingress_failure() {
    let config = LiveNodeConfig {
        delay_post_stop: Duration::ZERO,
        ..Default::default()
    };

    let mut node = LiveNodeBuilder::from_config(config)
        .unwrap()
        .with_external_ingress(Box::new(FailingIngress::default()))
        .build()
        .unwrap();
    node.kernel()
        .exec_engine
        .borrow_mut()
        .register_client(Box::new(LifecycleExecutionClient {
            state: LifecycleClientState::default(),
            behavior: LifecycleClientBehavior::ConnectPending,
        }))
        .unwrap();

    let (account_tx, account_rx) = tokio::sync::oneshot::channel();
    let account_tx = Cell::new(Some(account_tx));
    let handler = TypedHandler::from(move |_: &AccountState| {
        account_tx.take().unwrap().send(()).unwrap();
    });

    msgbus::register_account_state_endpoint(
        MessagingSwitchboard::portfolio_update_account(),
        handler,
    );

    let deferred_id = ClientOrderId::from("O-FROM-START");
    let queued_id = ClientOrderId::from("O-FROM-RUN");
    let sender = get_exec_event_sender();

    let send_order = |id| {
        let submitted = OrderSubmittedSpec::builder().client_order_id(id).build();
        sender
            .send(ExecutionEvent::Order(OrderEventAny::Submitted(submitted)))
            .unwrap();
    };

    send_order(deferred_id);

    let account = AccountState::new(
        AccountId::from("TEST-001"),
        AccountType::Cash,
        vec![],
        vec![],
        true,
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        None,
    );

    sender.send(ExecutionEvent::Account(account)).unwrap();

    tokio::time::timeout(Duration::from_secs(2), async {
        let mut start = Box::pin(node.start());
        tokio::select! {
            result = &mut start => panic!("Startup ended before cancellation: {result:?}"),
            account = account_rx => account.unwrap(),
        }
    })
    .await
    .unwrap();

    let received = Rc::new(RefCell::new(Vec::new()));

    let data_handler = TypedIntoHandler::from({
        let received = received.clone();
        move |_| received.borrow_mut().push(None)
    });

    msgbus::register_data_endpoint(
        MessagingSwitchboard::data_engine_process_data(),
        data_handler,
    );

    let order_handler = TypedIntoHandler::from({
        let received = received.clone();
        move |event: OrderEventAny| received.borrow_mut().push(Some(event.client_order_id()))
    });

    msgbus::register_order_event_endpoint(
        MessagingSwitchboard::exec_engine_process(),
        order_handler,
    );

    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let data = DataEvent::Data(instrument.into());
    get_data_event_sender().send(data).unwrap();
    send_order(queued_id);

    let error = node.run().await.unwrap_err();
    assert_eq!(error.to_string(), "external ingress receiver unavailable");
    assert_eq!(node.state(), NodeState::Stopped);
    assert_eq!(
        *received.borrow(),
        [None, Some(deferred_id), Some(queued_id)]
    );
}

#[rstest]
#[case::connection("timeout_connection", |config: &mut LiveNodeConfig, duration| config.timeout_connection = duration)]
#[case::disconnection("timeout_disconnection", |config: &mut LiveNodeConfig, duration| config.timeout_disconnection = duration)]
#[case::post_stop("delay_post_stop", |config: &mut LiveNodeConfig, duration| config.delay_post_stop = duration)]
fn test_node_build_rejects_overflowing_lifecycle_deadline(
    #[case] field: &str,
    #[case] configure: fn(&mut LiveNodeConfig, Duration),
    #[values(Duration::MAX, Duration::from_millis(u64::MAX))] duration: Duration,
) {
    let mut config = LiveNodeConfig::default();
    configure(&mut config, duration);
    let expected =
        format!("invalid LiveNodeConfig.{field}: duration exceeds the monotonic deadline range");

    let direct = LiveNode::build("InvalidDeadlineNode".to_string(), Some(config.clone()));
    assert_eq!(direct.unwrap_err().to_string(), expected);

    let builder = LiveNodeBuilder::from_config(config).unwrap();
    assert_eq!(builder.build().unwrap_err().to_string(), expected);
}

mod dst {
    use nautilus_common::live::dst::time;

    use super::*;

    #[rstest]
    #[case::manual(None)]
    #[case::hosted(Some(NodeRunMode::Hosted))]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_stop_during_data_connect_skips_execution_connect(
        #[case] mode: Option<NodeRunMode>,
    ) {
        let config = LiveNodeConfig {
            timeout_connection: Duration::ZERO,
            timeout_disconnection: Duration::ZERO,
            delay_post_stop: Duration::ZERO,
            ..Default::default()
        };

        let mut node = LiveNode::build("CancelledStartupNode".to_string(), Some(config)).unwrap();
        let handle = node.handle();
        let exec_state = LifecycleClientState::default();

        node.kernel().data_engine.borrow_mut().register_client(
            DataClientAdapter::new(
                ClientId::from("LIFECYCLE-DATA"),
                None,
                true,
                true,
                Box::new(LifecycleDataClient {
                    state: LifecycleClientState::default(),
                    behavior: LifecycleClientBehavior::Connects,
                    stop_on_connect: Some(handle.clone()),
                }),
            ),
            None,
        );

        node.kernel()
            .exec_engine
            .borrow_mut()
            .register_client(Box::new(LifecycleExecutionClient {
                state: exec_state.clone(),
                behavior: LifecycleClientBehavior::Connects,
            }))
            .unwrap();

        time::timeout(Duration::from_secs(2), async {
            match mode {
                None => node.start().await,
                Some(mode) => node.run_with_mode(mode).await,
            }
        })
        .await
        .expect("startup did not abort")
        .unwrap();

        assert!(!exec_state.connect_attempted.load(Ordering::Relaxed));
        assert_eq!(node.state(), NodeState::Stopped);
        assert!(handle.should_stop());
    }
}

#[cfg(not(madsim))]
#[tokio::test(start_paused = true)]
async fn test_explicit_startup_stop_returns_disconnect_failure_before_draining_commands() {
    use nautilus_common::runner::get_data_cmd_sender;

    use crate::dispatch_fixtures::stub_data_command;

    let config = LiveNodeConfig {
        timeout_connection: Duration::ZERO,
        timeout_disconnection: Duration::from_secs(1),
        delay_post_stop: Duration::ZERO,
        ..Default::default()
    };

    let mut node = LiveNode::build("StartupDisconnectFailure".to_string(), Some(config)).unwrap();
    let handle = node.handle();
    let state = LifecycleClientState::default();
    node.kernel().data_engine.borrow_mut().register_client(
        DataClientAdapter::new(
            ClientId::from("LIFECYCLE-DATA"),
            None,
            true,
            true,
            Box::new(LifecycleDataClient {
                state: state.clone(),
                behavior: LifecycleClientBehavior::DisconnectPending,
                stop_on_connect: Some(handle.clone()),
            }),
        ),
        None,
    );

    let commands = Rc::new(Cell::new(0));
    msgbus::register_data_command_endpoint(
        MessagingSwitchboard::data_engine_execute(),
        TypedIntoHandler::from({
            let commands = Rc::clone(&commands);
            move |_| commands.set(commands.get() + 1)
        }),
    );

    let run = node.run_with_mode(NodeRunMode::Hosted);
    tokio::pin!(run);
    assert!(futures_util::poll!(run.as_mut()).is_pending());
    assert!(state.disconnect_attempted.load(Ordering::Relaxed));
    assert_eq!(handle.state(), NodeState::ShuttingDown);
    get_data_cmd_sender().execute(stub_data_command());

    let error = run.await.unwrap_err();
    assert_eq!(
        error.to_string(),
        "disconnect timeout while disconnecting clients; failed while awaiting engine disconnection: disconnect readiness timeout while waiting for engine disconnections"
    );
    assert_eq!(handle.state(), NodeState::Stopped);
    assert_eq!(commands.get(), 0);
}

#[cfg(all(unix, not(madsim)))]
mod signals {
    use std::{
        io::{BufRead, BufReader},
        os::unix::process::ExitStatusExt,
        process::{Command, Stdio},
        sync::mpsc::{self, Receiver, RecvTimeoutError},
        thread,
        time::Instant,
    };

    use nautilus_common::{
        cache::{
            CacheConfig,
            database::{CacheDatabaseAdapter, CacheDatabaseFactory},
        },
        runner::get_data_cmd_sender,
        testing::wait_until_async,
    };

    use super::*;
    use crate::dispatch_fixtures::{stub_data_command, stub_data_event};

    const PHASE_ENV: &str = "NAUTILUS_LIVE_SIGNAL_TEST_PHASE";
    const READY: &str = "LiveNode signal test ready";
    const SHUTTING_DOWN: &str = "LiveNode signal test shutting down";

    #[rstest]
    #[case::interrupt_data_connect("data", "INT")]
    #[case::terminate_exec_connect("exec", "TERM")]
    #[case::terminate_exec_connect_with_closed_publisher("exec_closed_publisher", "TERM")]
    #[case::interrupt_reconciliation("reconciliation", "INT")]
    #[case::interrupt_shutdown_twice("shutdown", "INT")]
    #[case::hosted_keeps_signal_disposition("hosted", "TERM")]
    #[case::interrupt_cache_preparation("cache", "INT")]
    #[case::interrupt_pending_cache_preparation("cache_pending", "TERM")]
    #[case::interrupt_pending_cache_with_closed_publisher("cache_closed_publisher", "INT")]
    fn test_live_node_process_signals(#[case] phase: &str, #[case] signal: &str) {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "node::serial_tests::startup::signals::run_signal_test_child",
                "--ignored",
                "--nocapture",
            ])
            .env(PHASE_ENV, phase)
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (send, receive) = mpsc::channel();

        let output = thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if send.send(line).is_err() {
                    break;
                }
            }
        });

        let check = (|| -> anyhow::Result<()> {
            if phase != "cache" {
                wait_for_marker(&receive, READY)?;
                send_signal(child.id(), signal)?;
            }

            if phase == "shutdown" {
                wait_for_marker(&receive, SHUTTING_DOWN)?;
                send_signal(child.id(), signal)?;
            }

            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                match receive.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                    Ok(line) => println!("{line}"),
                    Err(RecvTimeoutError::Disconnected) => break Ok(()),
                    Err(RecvTimeoutError::Timeout) => {
                        anyhow::bail!("test child did not exit after SIG{signal}")
                    }
                }
            }
        })();

        if check.is_err() {
            let _ = child.kill();
        }

        let status = child.wait().unwrap();
        output.join().unwrap();
        check.unwrap();

        if phase == "hosted" {
            assert_eq!(status.signal(), Some(15));
        } else {
            assert!(status.success(), "test child exited with {status}");
        }
    }

    fn wait_for_marker(output: &Receiver<String>, marker: &str) -> anyhow::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(5);

        loop {
            let line = output
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .map_err(|e| anyhow::anyhow!("waiting for {marker}: {e}"))?;
            println!("{line}");

            if line.ends_with(marker) {
                return Ok(());
            }
        }
    }

    #[tokio::test]
    #[ignore = "subprocess target for the process signal tests"]
    async fn run_signal_test_child() {
        let Ok(phase) = std::env::var(PHASE_ENV) else {
            return;
        };

        if phase.starts_with("cache") {
            check_cache_preparation_interrupt(phase != "cache", phase == "cache_closed_publisher")
                .await;
            return;
        }

        let config = LiveNodeConfig {
            exec_engine: LiveExecutionEngineConfig {
                reconciliation: phase == "reconciliation",
                ..Default::default()
            },
            timeout_connection: Duration::from_secs(30),
            timeout_reconciliation: Duration::from_secs(30),
            timeout_disconnection: Duration::from_millis(250),
            delay_post_stop: if phase == "shutdown" {
                Duration::from_secs(2)
            } else {
                Duration::ZERO
            },
            ..Default::default()
        };

        let publisher_closed = phase == "exec_closed_publisher";
        let blocked_connection = phase == "data" || phase.starts_with("exec");
        let (ingress, ingress_closed) = signal_test_ingress(publisher_closed);
        let ingress_sender = ingress.sender.clone();
        let (mut node, pending, disconnected) =
            signal_test_node(&phase, config, blocked_connection.then_some(ingress));
        let handle = node.handle();

        let received_state = Rc::new(Cell::new(None));
        let commands = Rc::new(Cell::new(0));

        if blocked_connection {
            msgbus::register_data_endpoint(
                MessagingSwitchboard::data_engine_process_data(),
                TypedIntoHandler::from({
                    let handle = handle.clone();
                    let ingress_closed = Rc::clone(&ingress_closed);
                    let received_state = Rc::clone(&received_state);
                    move |_| {
                        assert!(
                            ingress_sender.is_closed(),
                            "ingress receiver must be released before dispatch"
                        );
                        assert!(
                            !ingress_closed.get(),
                            "ingress provider must stay open through dispatch"
                        );
                        received_state.set(Some(handle.state()));
                    }
                }),
            );

            msgbus::register_data_command_endpoint(
                MessagingSwitchboard::data_engine_execute(),
                TypedIntoHandler::from({
                    let commands = Rc::clone(&commands);
                    move |_| commands.set(commands.get() + 1)
                }),
            );
        }

        let mode = if phase == "hosted" {
            NodeRunMode::Hosted
        } else {
            NodeRunMode::Owned
        };

        let (result, ()) = tokio::join!(node.run_with_mode(mode), async {
            wait_until_async(
                || async {
                    if phase == "shutdown" {
                        handle.is_running()
                    } else {
                        pending.load(Ordering::Relaxed)
                    }
                },
                Duration::from_secs(3),
            )
            .await;

            if blocked_connection {
                get_data_event_sender().send(stub_data_event()).unwrap();
                get_data_cmd_sender().execute(stub_data_command());
            }

            println!("{READY}");
            if phase == "shutdown" {
                wait_until_async(
                    || async { handle.state() == NodeState::ShuttingDown },
                    Duration::from_secs(3),
                )
                .await;
                println!("{SHUTTING_DOWN}");
            }
        });

        result.unwrap();
        assert!(disconnected.load(Ordering::Relaxed));
        assert_eq!(handle.state(), NodeState::Stopped);

        if blocked_connection {
            assert!(ingress_closed.get());
            assert_eq!(received_state.get(), Some(NodeState::ShuttingDown));
            assert_eq!(commands.get(), 0);
        }
    }

    fn signal_test_node(
        phase: &str,
        config: LiveNodeConfig,
        ingress: Option<SignalTestIngress>,
    ) -> (LiveNode, Arc<AtomicBool>, Arc<AtomicBool>) {
        if phase == "reconciliation" {
            let (node, state) = live_node_with_startup_mass_status_client(
                "StartupSignalNode",
                config,
                StartupMassStatusBehavior::Pending,
            );

            return (
                node,
                state.mass_status_requested,
                state.disconnect_attempted,
            );
        }

        let mut builder = LiveNodeBuilder::from_config(config)
            .unwrap()
            .with_name("StartupSignalNode");

        if let Some(ingress) = ingress {
            builder = builder.with_external_ingress(Box::new(ingress));
        }

        let node = builder.build().unwrap();
        let state = LifecycleClientState::default();

        if phase.starts_with("exec") {
            node.kernel()
                .exec_engine
                .borrow_mut()
                .register_client(Box::new(LifecycleExecutionClient {
                    state: state.clone(),
                    behavior: LifecycleClientBehavior::ConnectPending,
                }))
                .unwrap();
        } else {
            node.kernel().data_engine.borrow_mut().register_client(
                DataClientAdapter::new(
                    ClientId::from("LIFECYCLE-DATA"),
                    None,
                    true,
                    true,
                    Box::new(LifecycleDataClient {
                        state: state.clone(),
                        behavior: if phase == "shutdown" {
                            LifecycleClientBehavior::Connects
                        } else {
                            LifecycleClientBehavior::ConnectPending
                        },
                        stop_on_connect: None,
                    }),
                ),
                None,
            );
        }

        (node, state.connect_attempted, state.disconnect_attempted)
    }

    async fn check_cache_preparation_interrupt(pending: bool, publisher_closed: bool) {
        let (ingress, closed) = signal_test_ingress(publisher_closed);

        let config = LiveNodeConfig {
            delay_post_stop: Duration::ZERO,
            ..Default::default()
        };

        let mut node = LiveNodeBuilder::from_config(config)
            .unwrap()
            .with_cache_database_factory(Box::new(InterruptingCacheFactory { pending }))
            .with_external_ingress(Box::new(ingress))
            .build()
            .unwrap();

        node.run().await.unwrap();

        assert_eq!(node.state(), NodeState::Stopped);
        assert!(closed.get());

        if pending {
            assert_eq!(node.kernel().ts_started(), None);
        }
    }

    #[derive(Debug)]
    struct InterruptingCacheFactory {
        pending: bool,
    }

    #[async_trait]
    impl CacheDatabaseFactory for InterruptingCacheFactory {
        async fn create(
            &self,
            _trader_id: TraderId,
            _instance_id: UUID4,
            _config: CacheConfig,
        ) -> anyhow::Result<Box<dyn CacheDatabaseAdapter>> {
            if self.pending {
                println!("{READY}");
                return std::future::pending().await;
            }

            send_signal(std::process::id(), "INT")?;
            let (database, _) = TestCacheDatabaseControl::create();
            Ok(Box::new(database))
        }
    }

    fn signal_test_ingress(publisher_closed: bool) -> (SignalTestIngress, Rc<Cell<bool>>) {
        let closed = Rc::new(Cell::new(false));
        let (sender, receiver) = tokio::sync::mpsc::channel(1);
        sender.try_send(BusMessage::new_close()).unwrap();

        let ingress = SignalTestIngress {
            sender,
            receiver: Some(receiver),
            closed: Rc::clone(&closed),
            publisher_closed,
        };

        (ingress, closed)
    }

    struct SignalTestIngress {
        sender: tokio::sync::mpsc::Sender<BusMessage>,
        receiver: Option<tokio::sync::mpsc::Receiver<BusMessage>>,
        closed: Rc<Cell<bool>>,
        publisher_closed: bool,
    }

    impl MessageBusExternalIngress for SignalTestIngress {
        fn is_closed(&self) -> bool {
            self.publisher_closed || self.closed.get()
        }

        fn take_receiver(&mut self) -> anyhow::Result<tokio::sync::mpsc::Receiver<BusMessage>> {
            self.receiver
                .take()
                .ok_or_else(|| anyhow::anyhow!("test ingress receiver already taken"))
        }

        fn close(&mut self) {
            assert!(
                self.sender.is_closed(),
                "ingress receiver must be released before close"
            );
            self.closed.set(true);
        }
    }

    fn send_signal(pid: u32, signal: &str) -> anyhow::Result<()> {
        let status = Command::new("kill")
            .args(["-s", signal, &pid.to_string()])
            .status()?;
        anyhow::ensure!(
            status.success(),
            "failed to send SIG{signal} to the test child"
        );
        Ok(())
    }
}

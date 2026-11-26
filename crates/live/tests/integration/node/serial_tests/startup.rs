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

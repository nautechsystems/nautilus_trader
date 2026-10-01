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

use nautilus_common::{
    component::component_state,
    enums::ComponentState,
    messages::execution::{CancelOrder, QueryOrder},
};
#[cfg(feature = "python")]
use pyo3::{prelude::*, types::PyDict};

use super::*;
#[cfg(feature = "python")]
use crate::python::node::PyLiveNode;

#[derive(Debug)]
struct ShutdownClient {
    connected: bool,
    retain: bool,
    disconnect_event: Rc<RefCell<Option<OrderEventAny>>>,
    submitted: Rc<RefCell<Vec<SubmitOrder>>>,
    queries: Rc<Cell<usize>>,
}

#[async_trait::async_trait(?Send)]
impl ExecutionClient for ShutdownClient {
    fn is_connected(&self) -> bool {
        self.connected
    }

    fn client_id(&self) -> ClientId {
        ClientId::from("SHUTDOWN")
    }

    fn account_id(&self) -> AccountId {
        AccountId::from("BINANCE-001")
    }

    fn venue(&self) -> Venue {
        crypto_perpetual_ethusdt().id().venue
    }

    fn oms_type(&self) -> OmsType {
        OmsType::Netting
    }

    fn get_account(&self) -> Option<AccountAny> {
        None
    }

    fn retain_unresolved_submissions(&self) -> bool {
        self.retain
    }

    fn generate_account_state(
        &self,
        _balances: Vec<AccountBalance>,
        _margins: Vec<MarginBalance>,
        _reported: bool,
        _ts_event: UnixNanos,
        _info: Option<Params>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn start(&mut self) -> anyhow::Result<()> {
        self.connected = true;
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        self.connected = false;
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        self.connected = false;

        if let Some(event) = self.disconnect_event.borrow_mut().take() {
            get_exec_event_sender().send(ExecutionEvent::Order(event))?;
        }
        Ok(())
    }

    fn submit_order(&self, command: SubmitOrder) -> anyhow::Result<()> {
        let mut order = OrderAny::try_from(command.order_init.clone())?;
        let submitted = TestOrderEventStubs::submitted(&order, self.account_id());
        order.apply(submitted.clone())?;
        order.apply(TestOrderEventStubs::accepted(
            &order,
            self.account_id(),
            VenueOrderId::from(format!("V-{}", order.client_order_id()).as_str()),
        ))?;
        let fill = OrderFilledTestBuilder::new(
            &order,
            &InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt()),
        )
        .position_id(command.position_id.expect("managed exit position"))
        .last_px(Price::from("100.00"))
        .commission(Money::zero(Currency::USDT()))
        .build();
        self.submitted.borrow_mut().push(command);
        get_exec_event_sender().send(ExecutionEvent::Order(submitted))?;
        get_exec_event_sender().send(ExecutionEvent::Order(fill))?;
        Ok(())
    }

    fn query_order(&self, _command: QueryOrder) -> anyhow::Result<()> {
        self.queries.set(self.queries.get() + 1);
        Ok(())
    }

    fn cancel_order(&self, _command: CancelOrder) -> anyhow::Result<()> {
        // An unacknowledged submission has no cancel outcome to report.
        Ok(())
    }
}

fn shutdown_node(
    policy: SubmissionRecoveryPolicy,
    client: ShutdownClient,
    manage_stop: bool,
) -> (LiveNode, OrderAny) {
    let config = LiveNodeConfig {
        exec_engine: crate::config::LiveExecutionEngineConfig {
            reconciliation: false,
            inflight_check_threshold_ms: 0,
            inflight_check_retries: 1,
            submission_recovery_policy: policy,
            ..Default::default()
        },
        timeout_connection: Duration::ZERO,
        timeout_reconciliation: Duration::ZERO,
        timeout_portfolio: Duration::ZERO,
        timeout_disconnection: Duration::from_millis(10),
        delay_post_stop: Duration::from_millis(if manage_stop { 250 } else { 50 }),
        timeout_shutdown: Duration::ZERO,
        ..Default::default()
    };
    let mut node = LiveNode::build("SubmissionShutdown".to_string(), Some(config)).unwrap();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let account_id = client.account_id();
    let client_id = client.client_id();
    let strategy_id = StrategyId::from("SHUTDOWN-001");
    node.exec_manager.register_submission_retention(&client);
    node.kernel
        .exec_engine
        .borrow_mut()
        .register_client(Box::new(client))
        .unwrap();
    node.kernel
        .exec_engine
        .borrow_mut()
        .register_venue_routing(client_id, instrument.id().venue)
        .unwrap();
    node.kernel
        .cache
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();
    node.kernel
        .cache
        .borrow_mut()
        .add_quote(QuoteTick::new(
            instrument.id(),
            Price::from("100.00"),
            Price::from("100.01"),
            Quantity::from("10.000"),
            Quantity::from("10.000"),
            UnixNanos::default(),
            UnixNanos::default(),
        ))
        .unwrap();
    node.kernel
        .cache
        .borrow_mut()
        .add_account(AccountAny::Margin(MarginAccount::new(
            AccountState::new(
                account_id,
                AccountType::Margin,
                vec![AccountBalance::new(
                    Money::from("1000000 USDT"),
                    Money::zero(Currency::USDT()),
                    Money::from("1000000 USDT"),
                )],
                Vec::new(),
                true,
                UUID4::new(),
                UnixNanos::default(),
                UnixNanos::default(),
                Some(Currency::USDT()),
            ),
            true,
        )))
        .unwrap();
    node.add_strategy(TestStrategy::new(StrategyConfig {
        strategy_id: Some(strategy_id),
        oms_type: Some(OmsType::Netting),
        manage_stop,
        market_exit_interval_ms: 5,
        market_exit_max_attempts: 100,
        ..Default::default()
    }))
    .unwrap();
    let order = OrderTestBuilder::new(OrderType::Limit)
        .trader_id(node.trader_id())
        .strategy_id(strategy_id)
        .client_order_id(ClientOrderId::from("O-SHUTDOWN"))
        .instrument_id(instrument.id())
        .quantity(Quantity::from("1.000"))
        .price(Price::from("100.00"))
        .build();
    node.observe_exec_command_before_dispatch(&TradingCommand::SubmitOrder(
        SubmitOrder::from_order(
            &order,
            node.trader_id(),
            Some(client_id),
            None,
            UUID4::new(),
            UnixNanos::default(),
        ),
    ));
    node.kernel
        .cache
        .borrow_mut()
        .add_order(order.clone(), None, Some(client_id), false)
        .unwrap();
    node.process_exec_event(ExecutionEvent::Order(TestOrderEventStubs::submitted(
        &order, account_id,
    )));
    (node, order)
}

#[rstest]
#[case::default(SubmissionRecoveryPolicy::ResolveLocally, false, false, false)]
#[case::active(SubmissionRecoveryPolicy::RetainUnresolved, false, false, false)]
#[case::exhausted(SubmissionRecoveryPolicy::RetainUnresolved, false, true, false)]
#[case::client_required(SubmissionRecoveryPolicy::ResolveLocally, true, true, false)]
#[case::after_deadline(SubmissionRecoveryPolicy::RetainUnresolved, false, true, true)]
#[tokio::test(start_paused = true)]
async fn test_submission_shutdown_boundary(
    #[values(false, true)] hosted: bool,
    #[case] policy: SubmissionRecoveryPolicy,
    #[case] client_retains: bool,
    #[case] exhaust: bool,
    #[case] evidence_on_disconnect: bool,
) {
    let queries = Rc::new(Cell::new(0));
    let disconnect_event = Rc::default();
    let (mut node, order) = shutdown_node(
        policy,
        ShutdownClient {
            connected: false,
            retain: client_retains,
            disconnect_event: Rc::clone(&disconnect_event),
            submitted: Rc::default(),
            queries: queries.clone(),
        },
        false,
    );

    if evidence_on_disconnect {
        *disconnect_event.borrow_mut() = Some(TestOrderEventStubs::accepted(
            &order,
            AccountId::from("BINANCE-001"),
            VenueOrderId::from("V-SHUTDOWN"),
        ));
    }

    if exhaust {
        tokio::time::advance(Duration::from_millis(1)).await;
        let result = node.exec_manager.check_inflight_orders();
        assert!(result.queries.is_empty());
        assert!(result.events.is_empty());
        assert_eq!(
            node.exec_manager
                .take_submission_recovery_exhaustions()
                .len(),
            1
        );
    }
    let start = tokio::time::Instant::now();
    let result = if hosted {
        let handle = node.handle();
        let stop = tokio::spawn(async move {
            while !handle.is_running() {
                tokio::task::yield_now().await;
            }
            handle.stop();
        });
        let result = node.run_with_mode(NodeRunMode::Hosted).await;
        stop.await.unwrap();
        result
    } else {
        node.start().await.unwrap();
        node.stop().await
    };

    if policy == SubmissionRecoveryPolicy::RetainUnresolved || client_retains {
        let error = result.unwrap_err().to_string();
        assert!(
            error.contains("Submission recovery incomplete at shutdown"),
            "{error}"
        );
        assert!(error.contains(order.client_order_id().as_str()), "{error}");
    } else {
        result.unwrap();
    }
    assert!(start.elapsed() < Duration::from_millis(200));
    assert_eq!(node.state(), NodeState::Stopped);
    assert!(node.kernel.check_engines_disconnected());
    assert_eq!(queries.get(), 0);
    assert_eq!(
        node.kernel
            .cache
            .borrow()
            .order(&order.client_order_id())
            .unwrap()
            .status(),
        if evidence_on_disconnect {
            OrderStatus::Accepted
        } else {
            OrderStatus::Submitted
        },
    );
    node.dispose();
}

#[rstest]
#[tokio::test]
async fn test_submission_shutdown_late_fill_completes_managed_exit(
    #[values(false, true)] hosted: bool,
) {
    let submitted = Rc::new(RefCell::new(Vec::new()));
    let (mut node, order) = shutdown_node(
        SubmissionRecoveryPolicy::RetainUnresolved,
        ShutdownClient {
            connected: false,
            retain: false,
            disconnect_event: Rc::default(),
            submitted: submitted.clone(),
            queries: Rc::default(),
        },
        true,
    );
    tokio::time::sleep(Duration::from_millis(1)).await;
    let exhausted = node.exec_manager.check_inflight_orders();
    assert!(exhausted.events.is_empty());
    assert!(exhausted.queries.is_empty());
    assert_eq!(
        node.exec_manager
            .take_submission_recovery_exhaustions()
            .len(),
        1
    );
    let fill = OrderFilledTestBuilder::new(
        &order,
        &InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt()),
    )
    .account_id(AccountId::from("BINANCE-001"))
    .without_position_id()
    .last_px(Price::from("100.00"))
    .commission(Money::zero(Currency::USDT()))
    .build();
    let handle = node.handle();

    if !hosted {
        node.start().await.unwrap();
    }
    let sender = get_exec_event_sender();

    let late_fill = tokio::spawn(async move {
        if hosted {
            while !handle.is_running() {
                tokio::task::yield_now().await;
            }
            handle.stop();
        }

        while handle.state() != NodeState::ShuttingDown {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
        assert_eq!(handle.state(), NodeState::ShuttingDown);
        sender.send(ExecutionEvent::Order(fill)).unwrap();
    });

    if hosted {
        node.run_with_mode(NodeRunMode::Hosted).await.unwrap();
    } else {
        node.stop().await.unwrap();
    }
    late_fill.await.unwrap();
    let commands = submitted.borrow();
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].order_init.order_side, OrderSide::Sell);
    assert!(commands[0].order_init.reduce_only);
    let cache = node.kernel.cache.borrow();
    assert_eq!(
        cache.order(&order.client_order_id()).unwrap().status(),
        OrderStatus::Filled
    );
    assert_eq!(
        cache.order(&commands[0].client_order_id).unwrap().status(),
        OrderStatus::Filled
    );
    assert_eq!(cache.positions_open_count(None, None, None, None, None), 0);
    assert_eq!(
        cache.positions_closed_count(None, None, None, None, None),
        1
    );
    assert_eq!(cache.orders_inflight_count(None, None, None, None, None), 0);
    assert!(node.exec_manager.unresolved_submission_ids().is_empty());
    assert_eq!(
        component_state(&order.strategy_id().inner()).unwrap(),
        ComponentState::Stopped,
    );
    drop(cache);
    assert_eq!(node.state(), NodeState::Stopped);
    node.dispose();
}

#[cfg(feature = "python")]
#[derive(Debug)]
struct FailingStopActor {
    core: DataActorCore,
}

#[cfg(feature = "python")]
nautilus_actor!(FailingStopActor);

#[cfg(feature = "python")]
impl DataActor for FailingStopActor {
    fn on_stop(&mut self) -> anyhow::Result<()> {
        let callback = TimeEventCallback::RustLocal(Rc::new(move |_| {
            crate::dispatch::tests::latch_callback_failure();
        }));
        nautilus_common::runner::get_time_event_sender().send(TimeEventMessage::new(
            TimeEvent::new(
                "shutdown-callback-failure".into(),
                UUID4::new(),
                UnixNanos::default(),
                UnixNanos::default(),
            ),
            callback,
        ));
        Ok(())
    }
}

#[cfg(feature = "python")]
#[rstest]
fn test_submission_shutdown_python_result(
    #[values("stop", "cancel", "timeout")] mode: &str,
    #[values(false, true)] retain: bool,
    #[values(false, true)] callback_failure: bool,
) {
    Python::initialize();
    let (mut node, order) = shutdown_node(
        if retain {
            SubmissionRecoveryPolicy::RetainUnresolved
        } else {
            SubmissionRecoveryPolicy::ResolveLocally
        },
        ShutdownClient {
            connected: false,
            retain: false,
            disconnect_event: Rc::default(),
            submitted: Rc::default(),
            queries: Rc::default(),
        },
        false,
    );

    if callback_failure {
        node.add_actor(FailingStopActor {
            core: DataActorCore::new(DataActorConfig {
                actor_id: Some(ActorId::from("SHUTDOWN-CALLBACK-FAILURE")),
                ..Default::default()
            }),
        })
        .unwrap();
    }
    let handle = node.handle();
    let cache = node.kernel.cache.clone();

    Python::attach(|py| {
        let locals = PyDict::new(py);
        locals.set_item("mode", mode).unwrap();
        locals.set_item("retain", retain).unwrap();
        locals
            .set_item("callback_failure", callback_failure)
            .unwrap();
        locals
            .set_item("node", Py::new(py, PyLiveNode::new(node)).unwrap())
            .unwrap();
        py.run(
            pyo3::ffi::c_str!(
                r#"
import asyncio

async def exercise():
    handle = node.handle()
    task = asyncio.create_task(node.run_async())

    while not handle.is_running:

        if task.done():
            await task
        await asyncio.sleep(0)

    try:

        if mode == "stop":
            handle.stop()
            await task
        elif mode == "cancel":
            task.cancel()
            await task
        else:
            async with asyncio.timeout(0):
                await task
    except BaseException as error:
        expected = {"stop": RuntimeError, "cancel": asyncio.CancelledError, "timeout": TimeoutError}
        assert isinstance(error, expected[mode]), repr(error)
        causes = []
        while error is not None:
            causes.append(error)
            error = error.__cause__
        incomplete = [e for e in causes if "Submission recovery incomplete at shutdown" in str(e)]
        assert bool(incomplete) == retain, repr(causes)
        callbacks = [e for e in causes if "Callback delivery unwound" in str(e)]
        assert bool(callbacks) == callback_failure, repr(causes)
        if retain:
            assert "O-SHUTDOWN" in str(incomplete[0])
    else:
        assert mode == "stop" and not retain and not callback_failure

    assert task.done()
    assert task.cancelled() == (mode != "stop")
    assert not handle.is_running

asyncio.run(exercise())
"#
            ),
            Some(&locals),
            None,
        )
        .unwrap();

        assert_eq!(handle.state(), NodeState::Stopped);
        assert_eq!(
            cache
                .borrow()
                .order(&order.client_order_id())
                .unwrap()
                .status(),
            OrderStatus::Submitted,
        );
        locals
            .get_item("node")
            .unwrap()
            .unwrap()
            .call_method0("dispose")
            .unwrap();
    });
}

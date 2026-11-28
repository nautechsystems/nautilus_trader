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

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use indexmap::IndexMap;
use nautilus_common::{
    clock::{Clock, VirtualClock},
    component::Component,
    enums::Environment,
    live::runner::get_exec_event_sender,
    messages::ExecutionEvent,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_live::{
    builder::LiveNodeBuilder,
    config::{LiveExecutionEngineConfig, LiveNodeConfig},
    node::{LiveNode, NodeState},
};
use nautilus_model::{
    enums::{OrderStatus, OrderType},
    identifiers::{AccountId, ActorId, InstrumentId, StrategyId, TraderId},
    orders::{Order, OrderTestBuilder, stubs::TestOrderEventStubs},
    types::Quantity,
};
use nautilus_system::event_store::EventStoreConfig;
use nautilus_testkit::{
    cache::TestCacheDatabaseControl,
    components::{StateActor, StateStrategy},
};
use nautilus_trading::strategy::StrategyConfig;
use rstest::rstest;

use super::TestStrategy;

#[rstest]
fn test_live_node_builder_clock_factory_drives_kernel_clock() {
    let calls = Rc::new(Cell::new(0_usize));
    let calls_in_factory = calls.clone();
    let sentinel = UnixNanos::from(123_456_789_u64);

    let node = LiveNode::builder(TraderId::from("TESTER-001"), Environment::Sandbox)
        .unwrap()
        .with_reconciliation(false)
        .with_clock_factory(move || {
            calls_in_factory.set(calls_in_factory.get() + 1);
            let mut clock = VirtualClock::new();
            clock.advance_time(sentinel, true);
            Rc::new(RefCell::new(clock)) as Rc<RefCell<dyn Clock>>
        })
        .build()
        .unwrap();

    assert_eq!(node.kernel().clock().borrow().timestamp_ns(), sentinel);
    assert_eq!(calls.get(), 1);
}

#[rstest]
#[tokio::test]
async fn test_start_stop_request_aborts_startup_without_running() {
    let config = LiveNodeConfig {
        exec_engine: LiveExecutionEngineConfig {
            reconciliation: false,
            ..Default::default()
        },
        timeout_disconnection: Duration::from_millis(50),
        ..Default::default()
    };

    let mut node = LiveNode::build("TestNode".to_string(), Some(config)).unwrap();
    let handle = node.handle();

    handle.stop();
    node.start().await.unwrap();

    assert_eq!(handle.state(), NodeState::Stopped);
    assert!(handle.should_stop());
    assert!(!handle.is_running());
}

#[rstest]
#[tokio::test(start_paused = true)]
async fn test_stop_processes_residual_exec_event_during_grace_period() {
    let config = LiveNodeConfig {
        exec_engine: LiveExecutionEngineConfig {
            reconciliation: false,
            ..Default::default()
        },
        timeout_connection: Duration::ZERO,
        timeout_reconciliation: Duration::ZERO,
        timeout_portfolio: Duration::ZERO,
        timeout_disconnection: Duration::ZERO,
        delay_post_stop: Duration::from_millis(20),
        ..Default::default()
    };

    let mut node = LiveNode::build("TestNode".to_string(), Some(config)).unwrap();
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(InstrumentId::from("EUR/USD.SIM"))
        .quantity(Quantity::from("2"))
        .build();
    let client_order_id = order.client_order_id();
    let submitted = TestOrderEventStubs::submitted(&order, AccountId::from("POLL-STOP-001"));

    node.kernel()
        .cache()
        .borrow_mut()
        .add_order(order, None, None, false)
        .unwrap();

    node.start().await.unwrap();
    let exec_event_sender = get_exec_event_sender();

    let send_residual = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(1)).await;
        exec_event_sender
            .send(ExecutionEvent::Order(submitted))
            .unwrap();
    });

    node.stop().await.unwrap();
    send_residual.await.unwrap();

    assert_eq!(
        node.kernel()
            .cache()
            .borrow()
            .order(&client_order_id)
            .unwrap()
            .status(),
        OrderStatus::Submitted
    );

    node.dispose();
}

#[rstest]
#[tokio::test]
async fn test_live_state_persistence_loads_before_start_and_saves_after_stop() {
    let actor_id = ActorId::from("LIVE-STATE-ACTOR");
    let strategy_id = StrategyId::from("LIVE-STATE-STRATEGY-001");
    let actor_load = IndexMap::from([("actor-load".to_string(), b"actor-loaded".to_vec())]);
    let strategy_load =
        IndexMap::from([("strategy-load".to_string(), b"strategy-loaded".to_vec())]);
    let actor_save = IndexMap::from([("actor-save".to_string(), b"actor-saved".to_vec())]);
    let strategy_save = IndexMap::from([("strategy-save".to_string(), b"strategy-saved".to_vec())]);
    let (database, control) = TestCacheDatabaseControl::create();
    control.set_actor_state(actor_id, &actor_load);
    control.set_strategy_state(strategy_id, &strategy_load);

    let config = LiveNodeConfig {
        load_state: true,
        save_state: true,
        exec_engine: LiveExecutionEngineConfig {
            reconciliation: false,
            ..Default::default()
        },
        timeout_connection: Duration::ZERO,
        timeout_reconciliation: Duration::ZERO,
        timeout_portfolio: Duration::ZERO,
        timeout_disconnection: Duration::ZERO,
        delay_post_stop: Duration::ZERO,
        ..Default::default()
    };

    let mut node = LiveNode::build("StatePersistenceNode".to_string(), Some(config)).unwrap();
    node.set_cache_database(Box::new(database)).unwrap();
    node.add_actor(StateActor::new(
        actor_id,
        control.clone(),
        actor_save.clone(),
    ))
    .unwrap();
    node.add_strategy(StateStrategy::new(
        strategy_id,
        control.clone(),
        strategy_save.clone(),
    ))
    .unwrap();

    node.start().await.unwrap();
    node.stop().await.unwrap();
    node.dispose();

    assert_eq!(
        control.events(),
        vec![
            "actor.load:LIVE-STATE-ACTOR",
            "actor.on_load",
            "strategy.load:LIVE-STATE-STRATEGY-001",
            "strategy.on_load",
            "actor.on_start",
            "strategy.on_start",
            "actor.on_stop",
            "strategy.on_stop",
            "actor.on_save",
            "actor.update:LIVE-STATE-ACTOR",
            "strategy.on_save",
            "strategy.update:LIVE-STATE-STRATEGY-001",
            "database.close",
        ]
    );
    assert_eq!(control.actor_state(&actor_id), Some(actor_save));
    assert_eq!(control.strategy_state(&strategy_id), Some(strategy_save));
    assert_eq!(node.state(), NodeState::Stopped);
}

#[rstest]
#[tokio::test]
async fn test_live_state_persistence_reports_callback_errors_after_shutdown() {
    let actor_id = ActorId::from("LIVE-FAIL-SAVE-ACTOR");
    let strategy_id = StrategyId::from("LIVE-FAIL-SAVE-STRATEGY-001");
    let (database, control) = TestCacheDatabaseControl::create();

    let config = LiveNodeConfig {
        save_state: true,
        exec_engine: LiveExecutionEngineConfig {
            reconciliation: false,
            ..Default::default()
        },
        timeout_connection: Duration::ZERO,
        timeout_reconciliation: Duration::ZERO,
        timeout_portfolio: Duration::ZERO,
        timeout_disconnection: Duration::ZERO,
        delay_post_stop: Duration::ZERO,
        ..Default::default()
    };

    let mut node = LiveNode::build("StatePersistenceErrorNode".to_string(), Some(config)).unwrap();
    node.set_cache_database(Box::new(database)).unwrap();
    node.add_actor(StateActor::new(actor_id, control.clone(), IndexMap::new()).with_fail_save())
        .unwrap();
    node.add_strategy(
        StateStrategy::new(strategy_id, control.clone(), IndexMap::new()).with_fail_save(),
    )
    .unwrap();

    node.start().await.unwrap();
    let error = node.stop().await.unwrap_err();
    node.dispose();

    assert_eq!(
        error.to_string(),
        "failed while finalizing kernel shutdown: Failed to save component state: actor \
         LIVE-FAIL-SAVE-ACTOR callback: test actor on_save failure; strategy \
         LIVE-FAIL-SAVE-STRATEGY-001 callback: test strategy on_save failure"
    );
    assert_eq!(
        control.events(),
        vec![
            "actor.on_start",
            "strategy.on_start",
            "actor.on_stop",
            "strategy.on_stop",
            "actor.on_save",
            "strategy.on_save",
            "database.close",
        ]
    );
    assert_eq!(node.state(), NodeState::Stopped);
}

#[rstest]
#[tokio::test]
async fn test_stop_drains_queued_exec_event_after_zero_grace() {
    let config = LiveNodeConfig {
        exec_engine: LiveExecutionEngineConfig {
            reconciliation: false,
            ..Default::default()
        },
        timeout_connection: Duration::ZERO,
        timeout_reconciliation: Duration::ZERO,
        timeout_portfolio: Duration::ZERO,
        timeout_disconnection: Duration::ZERO,
        delay_post_stop: Duration::ZERO,
        ..Default::default()
    };

    let mut node = LiveNode::build("TestNode".to_string(), Some(config)).unwrap();
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(InstrumentId::from("GBP/USD.SIM"))
        .quantity(Quantity::from("3"))
        .build();
    let client_order_id = order.client_order_id();
    let submitted = TestOrderEventStubs::submitted(&order, AccountId::from("POLL-DRAIN-001"));

    node.kernel()
        .cache()
        .borrow_mut()
        .add_order(order, None, None, false)
        .unwrap();

    node.start().await.unwrap();
    get_exec_event_sender()
        .send(ExecutionEvent::Order(submitted))
        .unwrap();

    node.stop().await.unwrap();

    assert_eq!(
        node.kernel()
            .cache()
            .borrow()
            .order(&client_order_id)
            .unwrap()
            .status(),
        OrderStatus::Submitted
    );

    node.dispose();
}

#[rstest]
fn test_dispose_before_start_is_idempotent() {
    let mut node = LiveNode::build("TestNode".to_string(), None).unwrap();
    node.add_strategy(TestStrategy::new(StrategyConfig {
        strategy_id: Some(StrategyId::from("DISPOSAL-001")),
        ..Default::default()
    }))
    .unwrap();

    node.dispose();
    node.dispose();

    assert!(node.kernel().trader().borrow().is_disposed());
    assert_eq!(node.kernel().trader().borrow().component_count(), 0);
    assert_eq!(node.state(), NodeState::Stopped);
}

#[rstest]
fn test_node_build_rejects_reentry_and_releases_process_after_factory_failure() {
    let result = LiveNodeBuilder::new(TraderId::default(), Environment::Live)
        .unwrap()
        .with_event_store(|_instance_id: UUID4, _clock: Rc<RefCell<dyn Clock>>| {
            let error = LiveNode::build("Reentrant".to_string(), None).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("already exists or is being built")
            );
            anyhow::bail!("Event store construction failed")
        })
        .build();

    assert_eq!(
        result.unwrap_err().to_string(),
        "Event store construction failed"
    );

    let node = LiveNode::build("Retry".to_string(), None).unwrap();
    assert_eq!(node.state(), NodeState::Idle);
}

#[rstest]
fn test_node_build_rejects_existing_node_on_another_thread(#[values(false, true)] builder: bool) {
    let node = LiveNode::build("First".to_string(), None).unwrap();

    let result = std::thread::spawn(move || {
        let result = if builder {
            LiveNodeBuilder::from_config(LiveNodeConfig::default())
                .unwrap()
                .build()
        } else {
            LiveNode::build("OtherThread".to_string(), None)
        };

        result.map(|other| other.state())
    })
    .join()
    .unwrap();

    assert_eq!(node.state(), NodeState::Idle);
    assert_eq!(
        result.unwrap_err().to_string(),
        "A LiveNode already exists or is being built in this process. Drop it before building another LiveNode"
    );
}

#[rstest]
fn test_node_build_releases_process_after_drop() {
    let mut node = LiveNode::build("First".to_string(), None).unwrap();
    node.dispose();
    assert!(LiveNode::build("WhileRetained".to_string(), None).is_err());
    drop(node);

    let replacement_state = std::thread::spawn(|| {
        LiveNode::build("Replacement".to_string(), None)
            .unwrap()
            .state()
    })
    .join()
    .unwrap();

    assert_eq!(replacement_state, NodeState::Idle);
}

#[rstest]
fn test_node_build_releases_process_after_failure(#[values(false, true)] builder: bool) {
    let config = LiveNodeConfig {
        event_store: Some(EventStoreConfig::default()),
        ..Default::default()
    };

    let result = if builder {
        LiveNodeBuilder::from_config(config).unwrap().build()
    } else {
        LiveNode::build("Failure".to_string(), Some(config))
    };

    assert!(result.unwrap_err().to_string().contains("factory"));

    let node = LiveNode::build("Retry".to_string(), None).unwrap();
    assert_eq!(node.state(), NodeState::Idle);
}

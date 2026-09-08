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

use std::{cell::RefCell, rc::Rc, sync::Arc};

use nautilus_common::{
    actor::{DataActor, DataActorCore, data_actor::DataActorConfig},
    cache::Cache,
    clients::DataClient,
    clock::TestClock,
    component::Component,
    messages::data::{DataCommand, SubscribeCustomData, UnsubscribeCustomData},
    msgbus::{self, MessageBus, TypedIntoHandler, switchboard::MessagingSwitchboard},
    nautilus_actor,
    runner::{SyncDataCommandSender, set_data_cmd_sender},
};
use nautilus_core::{Params, UUID4};
use nautilus_data::{client::DataClientAdapter, engine::DataEngine};
use nautilus_model::{
    data::DataType,
    identifiers::{ActorId, ClientId, TraderId, Venue},
};
use rstest::rstest;

#[rstest]
fn test_actor_subscribe_failure_requires_explicit_release_before_retry() {
    let mut fixture = SubscriptionFixture::new();
    fixture.client.borrow_mut().fail_subscribe = true;
    fixture.subscribe(0);
    fixture.subscribe(0);
    assert_eq!(fixture.client.borrow().subscribe_attempts.len(), 1);
    assert!(!fixture.client.borrow().active);
    assert!(fixture.engine.borrow().subscribed_custom_data().is_empty());

    fixture.actors[0].unsubscribe_data(fixture.data_type.clone(), Some(fixture.client_id), None);
    fixture.subscribe(0);
    fixture.subscribe(1);
    assert_eq!(fixture.client.borrow().subscribe_attempts.len(), 2);
    assert_eq!(
        fixture.engine.borrow().subscribed_custom_data(),
        vec![fixture.data_type.clone()]
    );

    let releases = fixture.client.borrow().unsubscribe_attempts.len();
    fixture.actors[0].dispose().unwrap();
    assert!(fixture.client.borrow().active);
    assert_eq!(fixture.client.borrow().unsubscribe_attempts.len(), releases);
    fixture.actors[1].dispose().unwrap();
    assert!(!fixture.client.borrow().active);
    assert_eq!(
        fixture.client.borrow().unsubscribe_attempts.len(),
        releases + 1
    );
    assert!(fixture.engine.borrow().subscribed_custom_data().is_empty());
}

#[rstest]
fn test_failed_actor_acquisition_retirement_preserves_successful_peer() {
    let mut fixture = SubscriptionFixture::new();
    fixture.client.borrow_mut().fail_subscribe = true;
    fixture.subscribe(0);
    fixture.subscribe(1);
    assert_eq!(fixture.client.borrow().subscribe_attempts.len(), 2);
    assert!(fixture.client.borrow().active);

    fixture.actors[0].dispose().unwrap();

    assert!(
        fixture.client.borrow().active,
        "retiring the failed acquisition must not stop the successful peer's feed"
    );
    assert!(fixture.client.borrow().unsubscribe_attempts.is_empty());
    assert_eq!(
        fixture.engine.borrow().subscribed_custom_data(),
        vec![fixture.data_type.clone()]
    );
    fixture.actors[1].dispose().unwrap();
    assert!(!fixture.client.borrow().active);
    assert_eq!(fixture.client.borrow().unsubscribe_attempts.len(), 1);
}

#[rstest]
fn test_actor_retirement_failure_preserves_feed_for_reacquisition() {
    let mut fixture = SubscriptionFixture::new();
    let original_params = fixture.subscribe(0);
    fixture.subscribe(1);
    fixture.client.borrow_mut().fail_unsubscribe = true;

    fixture.actors[0].dispose().unwrap();
    assert!(fixture.client.borrow().unsubscribe_attempts.is_empty());
    assert!(fixture.client.borrow().active);
    fixture.actors[1].dispose().unwrap();
    assert_eq!(fixture.client.borrow().unsubscribe_attempts.len(), 1);
    assert!(fixture.client.borrow().active);
    assert_eq!(
        fixture.engine.borrow().subscribed_custom_data(),
        vec![fixture.data_type.clone()]
    );

    fixture.actors[1].unsubscribe_data(fixture.data_type.clone(), Some(fixture.client_id), None);
    assert_eq!(fixture.client.borrow().unsubscribe_attempts.len(), 1);

    fixture.subscribe(2);
    assert_eq!(fixture.client.borrow().subscribe_attempts.len(), 1);
    fixture.actors[2].dispose().unwrap();

    let client = fixture.client.borrow();
    assert!(!client.active);
    assert_eq!(client.unsubscribe_attempts.len(), 2);
    for command in &client.unsubscribe_attempts {
        assert_eq!(command.data_type, fixture.data_type);
        assert_eq!(command.client_id, Some(fixture.client_id));
        assert_eq!(command.venue, None);
        assert_eq!(command.params.as_ref(), Some(&original_params));
    }
    assert!(fixture.engine.borrow().subscribed_custom_data().is_empty());
}

#[rstest]
fn test_engine_failed_reset_preserves_shared_acquisitions() {
    let mut fixture = SubscriptionFixture::new();
    fixture.subscribe(0);
    fixture.subscribe(1);
    fixture.client.borrow_mut().reset_behavior = ResetBehavior::Fail;

    fixture.engine.borrow_mut().reset();
    fixture.subscribe(2);
    assert_eq!(fixture.client.borrow().reset_attempts, 1);
    assert_eq!(fixture.client.borrow().subscribe_attempts.len(), 1);

    fixture.actors[0].dispose().unwrap();
    fixture.actors[1].dispose().unwrap();
    assert!(fixture.client.borrow().active);
    assert!(fixture.client.borrow().unsubscribe_attempts.is_empty());
    fixture.actors[2].dispose().unwrap();
    assert!(!fixture.client.borrow().active);
    assert_eq!(fixture.client.borrow().unsubscribe_attempts.len(), 1);
    assert!(fixture.engine.borrow().subscribed_custom_data().is_empty());
}

#[rstest]
#[case::success(ResetBehavior::Clear, false)]
#[case::failure(ResetBehavior::Fail, true)]
#[case::incomplete_success(ResetBehavior::Noop, true)]
fn test_engine_reset_subscription_state_follows_client_result(
    #[case] reset_behavior: ResetBehavior,
    #[case] active_after_reset: bool,
) {
    let mut fixture = SubscriptionFixture::new();
    fixture.subscribe(0);
    fixture.client.borrow_mut().reset_behavior = reset_behavior;
    fixture.engine.borrow_mut().reset();

    assert_eq!(fixture.client.borrow().reset_attempts, 1);
    assert_eq!(fixture.client.borrow().active, active_after_reset);
    let expected = if reset_behavior == ResetBehavior::Fail {
        vec![fixture.data_type.clone()]
    } else {
        Vec::new()
    };
    assert_eq!(fixture.engine.borrow().subscribed_custom_data(), expected);

    // Retire old actor intent before restart, including when reset failed physically
    fixture.actors[0].reset().unwrap();
    fixture.subscribe(0);
    fixture.subscribe(1);
    assert_eq!(fixture.client.borrow().subscribe_attempts.len(), 2);
    assert!(fixture.client.borrow().active);
    fixture.actors[0].dispose().unwrap();
    assert!(fixture.client.borrow().active);
    fixture.actors[1].dispose().unwrap();
    assert!(!fixture.client.borrow().active);
    assert!(fixture.engine.borrow().subscribed_custom_data().is_empty());
}

#[derive(Debug)]
struct SubscriptionActor {
    core: DataActorCore,
}

nautilus_actor!(SubscriptionActor);
impl DataActor for SubscriptionActor {}

struct SubscriptionFixture {
    _bus: Rc<RefCell<MessageBus>>,
    actors: Vec<SubscriptionActor>,
    engine: Rc<RefCell<DataEngine>>,
    client: Rc<RefCell<SubscriptionClientState>>,
    client_id: ClientId,
    data_type: DataType,
}

impl SubscriptionFixture {
    fn new() -> Self {
        let trader_id = TraderId::from("TRADER-001");
        let bus = MessageBus::new(trader_id, UUID4::new(), None, None).register_message_bus();
        set_data_cmd_sender(Arc::new(SyncDataCommandSender));
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));
        let engine = Rc::new(RefCell::new(DataEngine::new(
            clock.clone(),
            cache.clone(),
            None,
        )));
        let target = engine.clone();
        msgbus::register_data_command_endpoint(
            MessagingSwitchboard::data_engine_queue_execute(),
            TypedIntoHandler::from(move |command: DataCommand| {
                target.borrow_mut().execute(command);
            }),
        );
        let client_id = ClientId::from("SUBSCRIPTION-CLIENT");
        let client = Rc::new(RefCell::new(SubscriptionClientState::default()));
        engine.borrow_mut().register_client(
            DataClientAdapter::new(
                client_id,
                None,
                false,
                false,
                Box::new(SubscriptionClient {
                    client_id,
                    state: client.clone(),
                }),
            ),
            None,
        );
        let actors = (0..3)
            .map(|index| {
                let mut actor = SubscriptionActor {
                    core: DataActorCore::new(DataActorConfig {
                        actor_id: Some(ActorId::from(format!("SUBSCRIBER-{index}"))),
                        ..Default::default()
                    }),
                };
                actor
                    .register(trader_id, clock.clone(), cache.clone())
                    .unwrap();
                actor
            })
            .collect();
        Self {
            _bus: bus,
            actors,
            engine,
            client,
            client_id,
            data_type: DataType::new("LifecycleData", None, None),
        }
    }

    fn subscribe(&mut self, actor: usize) -> Params {
        let mut params = Params::new();
        params.insert("route".to_string(), serde_json::json!(actor + 37));
        self.actors[actor].subscribe_data(
            self.data_type.clone(),
            Some(self.client_id),
            Some(params.clone()),
        );
        params
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum ResetBehavior {
    #[default]
    Clear,
    Fail,
    Noop,
}

#[derive(Default)]
struct SubscriptionClientState {
    active: bool,
    fail_subscribe: bool,
    fail_unsubscribe: bool,
    reset_behavior: ResetBehavior,
    reset_attempts: usize,
    subscribe_attempts: Vec<SubscribeCustomData>,
    unsubscribe_attempts: Vec<UnsubscribeCustomData>,
}

struct SubscriptionClient {
    client_id: ClientId,
    state: Rc<RefCell<SubscriptionClientState>>,
}

impl DataClient for SubscriptionClient {
    fn client_id(&self) -> ClientId {
        self.client_id
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
    fn dispose(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
    fn is_connected(&self) -> bool {
        true
    }
    fn is_disconnected(&self) -> bool {
        false
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        let mut state = self.state.borrow_mut();
        state.reset_attempts += 1;
        match state.reset_behavior {
            ResetBehavior::Clear => state.active = false,
            ResetBehavior::Fail => anyhow::bail!("injected reset failure"),
            ResetBehavior::Noop => {}
        }
        Ok(())
    }

    fn subscribe(&mut self, command: SubscribeCustomData) -> anyhow::Result<()> {
        let mut state = self.state.borrow_mut();
        state.subscribe_attempts.push(command);
        if std::mem::take(&mut state.fail_subscribe) {
            anyhow::bail!("injected subscribe failure");
        }
        state.active = true;
        Ok(())
    }

    fn unsubscribe(&mut self, command: &UnsubscribeCustomData) -> anyhow::Result<()> {
        let mut state = self.state.borrow_mut();
        state.unsubscribe_attempts.push(command.clone());
        if std::mem::take(&mut state.fail_unsubscribe) {
            anyhow::bail!("injected unsubscribe failure");
        }
        state.active = false;
        Ok(())
    }
}

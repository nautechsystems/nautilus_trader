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

//! Shared DeFi subscription demand across actors, subscription shapes, and clients.

use std::{cell::RefCell, rc::Rc, sync::Arc};

use nautilus_common::{
    actor::{DataActor, DataActorCore, data_actor::DataActorConfig},
    cache::Cache,
    clients::DataClient,
    clock::{Clock, VirtualClock},
    component::Component,
    messages::{
        data::DataCommand,
        defi::{
            DefiRequestCommand, DefiSubscribeCommand, DefiUnsubscribeCommand, PoolSnapshotResponse,
            SubscribePoolSwaps, UnsubscribePoolSwaps,
        },
    },
    msgbus::{self, MessageBus, TypedIntoHandler, switchboard::MessagingSwitchboard},
    nautilus_actor,
    runner::{SyncDataCommandSender, set_data_cmd_sender},
};
use nautilus_core::UUID4;
use nautilus_data::{client::DataClientAdapter, engine::DataEngine};
use nautilus_model::{
    defi::{
        DefiData, Pool, PoolProfiler, PoolSwap, data::DexPoolData, pool_analysis::PoolSnapshot,
    },
    identifiers::{ActorId, ClientId, InstrumentId, TraderId, Venue},
};
use rstest::rstest;

use crate::common::{defi::make_initialized_pool_and_swap, mocks::MockDataClient};

#[rstest]
#[case::first_subscriber_retires_first(0)]
#[case::second_subscriber_retires_first(1)]
fn test_shared_pool_demand_survives_until_final_owner_retires(#[case] first: usize) {
    let mut fixture = DefiFixture::new(true);
    let client_id = fixture.client_ids[0];
    fixture.subscribe(0, PoolShape::Swaps, client_id);
    fixture.subscribe(1, PoolShape::Swaps, client_id);

    fixture.actors[first].dispose().unwrap();
    let applied_after_first = fixture.updater_applies_swap();
    let unsubscribes_after_first = fixture.unsubscribes(0);

    fixture.actors[1 - first].dispose().unwrap();
    let applied_after_final = fixture.updater_applies_swap();

    assert_eq!(fixture.subscribes(0), vec![PoolShape::Swaps]);
    assert!(applied_after_first);
    assert!(unsubscribes_after_first.is_empty());
    assert!(!applied_after_final);
    assert_eq!(fixture.unsubscribes(0), vec![PoolShape::Swaps]);
}

#[rstest]
fn test_duplicate_unsubscribe_preserves_peer_demand() {
    let mut fixture = DefiFixture::new(true);
    let client_id = fixture.client_ids[0];
    fixture.subscribe(0, PoolShape::Swaps, client_id);
    fixture.subscribe(1, PoolShape::Swaps, client_id);

    fixture.unsubscribe(0, PoolShape::Swaps, client_id);
    fixture.unsubscribe(0, PoolShape::Swaps, client_id);

    assert!(fixture.updater_applies_swap());
    assert!(fixture.unsubscribes(0).is_empty());
}

#[rstest]
fn test_subscription_without_client_sets_up_no_updater() {
    let mut fixture = DefiFixture::new(true);

    fixture.subscribe(0, PoolShape::Swaps, ClientId::from("DEFI-MISSING"));

    assert!(!fixture.has_profiler());
    assert!(fixture.subscribes(0).is_empty());
    assert!(fixture.subscribes(1).is_empty());
}

#[rstest]
fn test_failed_final_client_unsubscribe_releases_updater() {
    let mut fixture = DefiFixture::new(true);
    let client_id = ClientId::from("DEFI-FAILING-UNSUBSCRIBE");
    fixture.engine.borrow_mut().register_client(
        DataClientAdapter::new(
            client_id,
            None,
            false,
            false,
            Box::new(FailingUnsubscribeClient { client_id }),
        ),
        None,
    );
    fixture.subscribe(0, PoolShape::Swaps, client_id);
    let applied_before = fixture.updater_applies_swap();

    fixture.unsubscribe(0, PoolShape::Swaps, client_id);

    assert!(applied_before);
    assert!(!fixture.updater_applies_swap());
}

#[rstest]
fn test_complete_pool_overlaps_narrow_shape(
    #[values(
        PoolShape::Swaps,
        PoolShape::LiquidityUpdates,
        PoolShape::FeeCollects,
        PoolShape::FlashEvents
    )]
    narrow: PoolShape,
    #[values(true, false)] pool_first: bool,
) {
    let mut fixture = DefiFixture::new(true);
    let client_id = fixture.client_ids[0];
    fixture.subscribe(0, PoolShape::Pool, client_id);
    fixture.subscribe(1, narrow, client_id);

    let (first, second) = if pool_first {
        ((0, PoolShape::Pool), (1, narrow))
    } else {
        ((1, narrow), (0, PoolShape::Pool))
    };

    fixture.unsubscribe(first.0, first.1, client_id);
    let applied_after_first = fixture.updater_applies_swap();
    let unsubscribes_after_first = fixture.unsubscribes(0);

    fixture.unsubscribe(second.0, second.1, client_id);
    let applied_after_final = fixture.updater_applies_swap();

    assert!(applied_after_first);
    assert_eq!(unsubscribes_after_first, vec![first.1]);
    assert!(!applied_after_final);
    assert_eq!(fixture.unsubscribes(0), vec![first.1, second.1]);
}

#[rstest]
fn test_pool_demand_is_independent_per_client() {
    let mut fixture = DefiFixture::new(true);
    let (client_a, client_b) = (fixture.client_ids[0], fixture.client_ids[1]);
    fixture.subscribe(0, PoolShape::Swaps, client_a);
    fixture.subscribe(1, PoolShape::Swaps, client_b);

    fixture.unsubscribe(0, PoolShape::Swaps, client_a);
    let applied_after_first = fixture.updater_applies_swap();
    let client_b_after_first = fixture.unsubscribes(1);

    fixture.unsubscribe(1, PoolShape::Swaps, client_b);
    let applied_after_final = fixture.updater_applies_swap();

    assert_eq!(fixture.subscribes(0), vec![PoolShape::Swaps]);
    assert_eq!(fixture.subscribes(1), vec![PoolShape::Swaps]);
    assert_eq!(fixture.unsubscribes(0), vec![PoolShape::Swaps]);
    assert!(applied_after_first);
    assert!(client_b_after_first.is_empty());
    assert!(!applied_after_final);
    assert_eq!(fixture.unsubscribes(1), vec![PoolShape::Swaps]);
}

#[rstest]
fn test_canceled_bootstrap_response_cannot_activate_later_subscription() {
    let mut fixture = DefiFixture::new(false);
    let client_id = fixture.client_ids[0];
    let snapshot = fixture.snapshot();
    let snapshot_block = snapshot.block_position.number;
    fixture.subscribe(0, PoolShape::Swaps, client_id);
    fixture.publish_swap_at(snapshot_block + 1);
    fixture.unsubscribe(0, PoolShape::Swaps, client_id);
    fixture.subscribe(1, PoolShape::Swaps, client_id);
    fixture.publish_swap_at(snapshot_block);
    fixture.publish_swap_at(snapshot_block + 2);
    let requests = fixture.snapshot_requests(0);
    fixture
        .engine
        .borrow_mut()
        .process_defi_data(DefiData::Pool(fixture.pool.clone()));

    fixture.respond(requests[0], snapshot.clone());
    let installed_by_stale = fixture.has_profiler();
    fixture.respond(requests[1], snapshot.clone());

    assert_eq!(requests.len(), 2);
    assert_ne!(requests[0], requests[1]);
    assert!(!installed_by_stale);
    assert_eq!(fixture.profiler_last_block(), Some(snapshot_block + 2));
    // Only the current bootstrap's post-snapshot swap applies; the canceled buffer is gone
    assert_eq!(
        fixture.profiler_total_swaps(),
        Some(snapshot.analytics.total_swaps + 1)
    );
    assert!(fixture.updater_applies_swap());
}

#[rstest]
fn test_reset_discards_pending_bootstrap() {
    let mut fixture = DefiFixture::new(false);
    fixture.subscribe(0, PoolShape::Swaps, fixture.client_ids[0]);

    fixture.engine.borrow_mut().reset();
    let installed_by_stale = fixture.answer_first_request_then_resubscribe();

    assert!(!installed_by_stale);
    assert!(fixture.has_profiler());
    assert!(fixture.updater_applies_swap());
}

#[rstest]
#[tokio::test]
#[expect(clippy::await_holding_refcell_ref)] // Single-threaded test
async fn test_disconnect_discards_pending_bootstrap() {
    let mut fixture = DefiFixture::new(false);
    fixture.subscribe(0, PoolShape::Swaps, fixture.client_ids[0]);

    fixture.engine.borrow_mut().disconnect().await.unwrap();
    let installed_by_stale = fixture.answer_first_request_then_resubscribe();

    assert!(!installed_by_stale);
    assert!(fixture.has_profiler());
    assert!(fixture.updater_applies_swap());
}

#[rstest]
fn test_reset_releases_active_pool_updater() {
    let mut fixture = DefiFixture::new(true);
    let client_id = fixture.client_ids[0];
    fixture.subscribe(0, PoolShape::Swaps, client_id);
    let applied_before = fixture.updater_applies_swap();

    fixture.engine.borrow_mut().reset();

    assert!(applied_before);
    assert!(!fixture.updater_applies_swap());
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PoolShape {
    Pool,
    Swaps,
    LiquidityUpdates,
    FeeCollects,
    FlashEvents,
}

impl PoolShape {
    fn from_subscribe(command: &DefiSubscribeCommand) -> Option<Self> {
        match command {
            DefiSubscribeCommand::Pool(_) => Some(Self::Pool),
            DefiSubscribeCommand::PoolSwaps(_) => Some(Self::Swaps),
            DefiSubscribeCommand::PoolLiquidityUpdates(_) => Some(Self::LiquidityUpdates),
            DefiSubscribeCommand::PoolFeeCollects(_) => Some(Self::FeeCollects),
            DefiSubscribeCommand::PoolFlashEvents(_) => Some(Self::FlashEvents),
            DefiSubscribeCommand::Blocks(_) => None,
        }
    }

    fn from_unsubscribe(command: &DefiUnsubscribeCommand) -> Option<Self> {
        match command {
            DefiUnsubscribeCommand::Pool(_) => Some(Self::Pool),
            DefiUnsubscribeCommand::PoolSwaps(_) => Some(Self::Swaps),
            DefiUnsubscribeCommand::PoolLiquidityUpdates(_) => Some(Self::LiquidityUpdates),
            DefiUnsubscribeCommand::PoolFeeCollects(_) => Some(Self::FeeCollects),
            DefiUnsubscribeCommand::PoolFlashEvents(_) => Some(Self::FlashEvents),
            DefiUnsubscribeCommand::Blocks(_) => None,
        }
    }
}

/// Accepts DeFi subscriptions and rejects every DeFi unsubscribe.
struct FailingUnsubscribeClient {
    client_id: ClientId,
}

impl DataClient for FailingUnsubscribeClient {
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
    fn reset(&mut self) -> anyhow::Result<()> {
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

    fn subscribe_pool_swaps(&mut self, _cmd: SubscribePoolSwaps) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_pool_swaps(&mut self, _cmd: &UnsubscribePoolSwaps) -> anyhow::Result<()> {
        anyhow::bail!("injected unsubscribe failure")
    }
}

#[derive(Debug)]
struct DefiActor {
    core: DataActorCore,
}

nautilus_actor!(DefiActor);
impl DataActor for DefiActor {}

struct DefiFixture {
    _bus: Rc<RefCell<MessageBus>>,
    actors: Vec<DefiActor>,
    engine: Rc<RefCell<DataEngine>>,
    client_ids: Vec<ClientId>,
    recorders: Vec<Rc<RefCell<Vec<DataCommand>>>>,
    pool: Pool,
    swap: PoolSwap,
}

impl DefiFixture {
    fn new(pool_cached: bool) -> Self {
        let trader_id = TraderId::from("TRADER-001");
        let bus = MessageBus::new(trader_id, UUID4::new(), None, None).register_message_bus();
        set_data_cmd_sender(Arc::new(SyncDataCommandSender));
        let clock = Rc::new(RefCell::new(VirtualClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));

        let engine = Rc::new(RefCell::new(DataEngine::new(
            Rc::clone(&clock) as Rc<RefCell<dyn Clock>>,
            Rc::clone(&cache),
            None,
        )));
        let target = Rc::clone(&engine);
        msgbus::register_data_command_endpoint(
            MessagingSwitchboard::data_engine_queue_execute(),
            TypedIntoHandler::from(move |command: DataCommand| {
                target.borrow_mut().execute(command);
            }),
        );

        let client_ids = vec![ClientId::from("DEFI-A"), ClientId::from("DEFI-B")];

        let recorders: Vec<_> = client_ids
            .iter()
            .map(|client_id| {
                let recorder = Rc::new(RefCell::new(Vec::new()));
                let client = MockDataClient::new_with_recorder(
                    Rc::clone(&clock) as Rc<RefCell<dyn Clock>>,
                    Rc::clone(&cache),
                    *client_id,
                    None,
                    Some(Rc::clone(&recorder)),
                );
                engine.borrow_mut().register_client(
                    DataClientAdapter::new(*client_id, None, false, false, Box::new(client)),
                    None,
                );
                recorder
            })
            .collect();

        let actors = (0..2)
            .map(|index| {
                let mut actor = DefiActor {
                    core: DataActorCore::new(DataActorConfig {
                        actor_id: Some(ActorId::from(format!("DEFI-SUBSCRIBER-{index}"))),
                        ..Default::default()
                    }),
                };

                actor
                    .register(
                        trader_id,
                        Rc::clone(&clock) as Rc<RefCell<dyn Clock>>,
                        Rc::clone(&cache),
                    )
                    .unwrap();
                actor
            })
            .collect();

        let (pool, swap) = make_initialized_pool_and_swap();

        if pool_cached {
            cache.borrow_mut().add_pool(pool.clone()).unwrap();
        }

        Self {
            _bus: bus,
            actors,
            engine,
            client_ids,
            recorders,
            pool,
            swap,
        }
    }

    fn instrument_id(&self) -> InstrumentId {
        self.pool.instrument_id
    }

    fn subscribe(&mut self, actor: usize, shape: PoolShape, client_id: ClientId) {
        let instrument_id = self.instrument_id();
        let actor = &mut self.actors[actor];
        let client_id = Some(client_id);

        match shape {
            PoolShape::Pool => actor.subscribe_pool(instrument_id, client_id, None),
            PoolShape::Swaps => actor.subscribe_pool_swaps(instrument_id, client_id, None),
            PoolShape::LiquidityUpdates => {
                actor.subscribe_pool_liquidity_updates(instrument_id, client_id, None);
            }
            PoolShape::FeeCollects => {
                actor.subscribe_pool_fee_collects(instrument_id, client_id, None);
            }
            PoolShape::FlashEvents => {
                actor.subscribe_pool_flash_events(instrument_id, client_id, None);
            }
        }
    }

    fn unsubscribe(&mut self, actor: usize, shape: PoolShape, client_id: ClientId) {
        let instrument_id = self.instrument_id();
        let actor = &mut self.actors[actor];
        let client_id = Some(client_id);

        match shape {
            PoolShape::Pool => actor.unsubscribe_pool(instrument_id, client_id, None),
            PoolShape::Swaps => actor.unsubscribe_pool_swaps(instrument_id, client_id, None),
            PoolShape::LiquidityUpdates => {
                actor.unsubscribe_pool_liquidity_updates(instrument_id, client_id, None);
            }
            PoolShape::FeeCollects => {
                actor.unsubscribe_pool_fee_collects(instrument_id, client_id, None);
            }
            PoolShape::FlashEvents => {
                actor.unsubscribe_pool_flash_events(instrument_id, client_id, None);
            }
        }
    }

    fn subscribes(&self, client: usize) -> Vec<PoolShape> {
        self.recorders[client]
            .borrow()
            .iter()
            .filter_map(|command| match command {
                DataCommand::DefiSubscribe(command) => PoolShape::from_subscribe(command),
                _ => None,
            })
            .collect()
    }

    fn unsubscribes(&self, client: usize) -> Vec<PoolShape> {
        self.recorders[client]
            .borrow()
            .iter()
            .filter_map(|command| match command {
                DataCommand::DefiUnsubscribe(command) => PoolShape::from_unsubscribe(command),
                _ => None,
            })
            .collect()
    }

    fn snapshot_requests(&self, client: usize) -> Vec<UUID4> {
        self.recorders[client]
            .borrow()
            .iter()
            .filter_map(|command| match command {
                DataCommand::DefiRequest(DefiRequestCommand::PoolSnapshot(request)) => {
                    Some(request.request_id)
                }
                _ => None,
            })
            .collect()
    }

    fn has_profiler(&self) -> bool {
        self.engine
            .borrow()
            .cache()
            .borrow()
            .pool_profiler(&self.instrument_id())
            .is_some()
    }

    fn profiler_total_swaps(&self) -> Option<u64> {
        self.engine
            .borrow()
            .cache()
            .borrow()
            .pool_profiler(&self.instrument_id())
            .map(|profiler| profiler.analytics.total_swaps)
    }

    fn profiler_last_block(&self) -> Option<u64> {
        self.engine
            .borrow()
            .cache()
            .borrow()
            .pool_profiler(&self.instrument_id())
            .and_then(|profiler| profiler.last_processed_event.as_ref())
            .map(|position| position.number)
    }

    /// Builds a usable snapshot positioned at the fixture swap.
    fn snapshot(&self) -> PoolSnapshot {
        let mut profiler = PoolProfiler::new(Arc::new(self.pool.clone()));
        profiler
            .initialize(self.pool.initial_sqrt_price_x96.unwrap())
            .unwrap();
        profiler
            .process(&DexPoolData::Swap(self.swap.clone()))
            .unwrap();
        profiler.extract_snapshot().unwrap()
    }

    fn respond(&self, correlation_id: UUID4, snapshot: PoolSnapshot) {
        let response = PoolSnapshotResponse::new(correlation_id, snapshot);
        self.engine.borrow_mut().process(&response);
    }

    /// Answers the first snapshot request after the pool arrives, then subscribes a second
    /// actor, which builds its profiler from the cached pool. Returns whether the answer
    /// installed a profiler.
    fn answer_first_request_then_resubscribe(&mut self) -> bool {
        let snapshot = self.snapshot();
        self.engine
            .borrow_mut()
            .process_defi_data(DefiData::Pool(self.pool.clone()));

        self.respond(self.snapshot_requests(0)[0], snapshot);
        let installed_by_answer = self.has_profiler();
        self.subscribe(1, PoolShape::Swaps, self.client_ids[0]);
        installed_by_answer
    }

    /// Publishes a swap after every previously applied event and reports whether the
    /// engine's pool updater applied it to the cached profiler.
    fn updater_applies_swap(&mut self) -> bool {
        let block = self.swap.block + 1;
        self.publish_swap_at(block);
        self.profiler_last_block() == Some(block)
    }

    fn publish_swap_at(&mut self, block: u64) {
        self.swap.block = self.swap.block.max(block);
        let mut swap = self.swap.clone();
        swap.block = block;
        self.engine
            .borrow_mut()
            .process_defi_data(DefiData::PoolSwap(swap));
    }
}

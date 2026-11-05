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

use std::{cell::RefCell, rc::Rc};

use nautilus_common::{
    actor::DataActor,
    cache::Cache,
    clock::{Clock, VirtualClock},
    component::Component,
    messages::data::{DataCommand, SubscribeCommand, UnsubscribeCommand},
    msgbus::{
        self, MessageBus, MessagingSwitchboard, get_message_bus,
        stubs::get_typed_into_message_saving_handler, switchboard::get_book_deltas_topic,
    },
};
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{BookOrder, OrderBookDelta, OrderBookDeltas},
    enums::{BookAction, BookType, OrderSide},
    identifiers::{ActorId, InstrumentId, TraderId},
    types::{Price, Quantity},
};
use rstest::rstest;

use super::*;

fn make_delta(
    instrument_id: InstrumentId,
    side: OrderSide,
    price: &str,
    size: &str,
    ts: u64,
) -> OrderBookDelta {
    let order = BookOrder::new(side, Price::from(price), Quantity::from(size), 1);
    OrderBookDelta::new(
        instrument_id,
        BookAction::Add,
        order,
        0,
        0,
        UnixNanos::from(ts),
        UnixNanos::from(ts),
    )
}

fn make_deltas(instrument_id: InstrumentId, deltas: Vec<OrderBookDelta>) -> OrderBookDeltas {
    OrderBookDeltas::new(instrument_id, deltas)
}

fn instrument_a() -> InstrumentId {
    InstrumentId::from("1.123-100.BETFAIR")
}

fn instrument_b() -> InstrumentId {
    InstrumentId::from("1.123-200.BETFAIR")
}

fn register_actor(actor: &mut BookImbalanceActor) {
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache = Rc::new(RefCell::new(Cache::default()));
    actor
        .register(TraderId::from("TESTER-001"), clock, cache)
        .unwrap();
}

#[rstest]
fn test_new_actor_starts_with_empty_states() {
    let actor = BookImbalanceActor::new(vec![instrument_a()], 0, None);

    assert!(actor.states().is_empty());
    assert_eq!(actor.actor_id(), ActorId::from("BOOK_IMBALANCE-001"));
}

#[rstest]
#[case::single_stop(1, false)]
#[case::restart_after_reset(2, false)]
#[case::resume_after_stop(2, true)]
fn test_stop_releases_each_book_subscription(#[case] cycles: usize, #[case] resume: bool) {
    *get_message_bus().borrow_mut() = MessageBus::default();
    let ids = [instrument_a(), InstrumentId::from("BTCUSDT.BINANCE")];
    let mut actor = BookImbalanceActor::new(ids.to_vec(), 0, None);
    register_actor(&mut actor);
    let (handler, saver) = get_typed_into_message_saving_handler::<DataCommand>(None);
    msgbus::register_data_command_endpoint(
        MessagingSwitchboard::data_engine_queue_execute(),
        handler,
    );

    for cycle in 0..cycles {
        if cycle > 0 && !resume {
            actor.reset().unwrap();
        }

        saver.clear();

        if cycle > 0 && resume {
            actor.resume().unwrap();
        } else {
            actor.start().unwrap();
        }

        for id in ids {
            let topic = get_book_deltas_topic(id);
            assert_eq!(msgbus::subscriber_count_deltas(topic), 1);
        }

        let deltas = make_deltas(
            ids[0],
            vec![
                make_delta(ids[0], OrderSide::Buy, "2.50", "13", 1_000_000),
                make_delta(ids[0], OrderSide::Sell, "2.52", "7", 1_000_000),
            ],
        );
        actor.on_book_deltas(&deltas).unwrap();
        actor.stop().unwrap();

        let commands = saver.get_messages();
        assert_eq!(commands.len(), 2 * ids.len());

        for (index, id) in ids.iter().enumerate() {
            let DataCommand::Subscribe(SubscribeCommand::BookDeltas(subscribe)) = &commands[index]
            else {
                panic!("expected book deltas subscribe, was {:?}", commands[index]);
            };

            let DataCommand::Unsubscribe(UnsubscribeCommand::BookDeltas(unsubscribe)) =
                &commands[index + ids.len()]
            else {
                panic!(
                    "expected book deltas unsubscribe, was {:?}",
                    commands[index + ids.len()]
                );
            };

            assert_eq!(subscribe.instrument_ids, vec![*id]);
            assert_eq!(subscribe.book_type, BookType::L2_MBP);
            assert_eq!(subscribe.depth, None);
            assert_eq!(subscribe.client_id, None);
            assert_eq!(subscribe.venue, Some(id.venue));
            assert!(!subscribe.managed);
            assert_eq!(subscribe.ts_init, UnixNanos::default());
            assert_eq!(subscribe.correlation_id, None);
            assert_eq!(subscribe.params, None);
            assert_eq!(unsubscribe.instrument_ids, subscribe.instrument_ids);
            assert_eq!(unsubscribe.client_id, subscribe.client_id);
            assert_eq!(unsubscribe.venue, subscribe.venue);
            assert_eq!(unsubscribe.ts_init, UnixNanos::default());
            assert_eq!(unsubscribe.correlation_id, Some(subscribe.command_id));
            assert_eq!(unsubscribe.params, subscribe.params);
        }

        assert!(actor.is_stopped());

        let update_count = if resume { cycle as u64 + 1 } else { 1 };
        let state = &actor.states()[&ids[0]];
        assert_eq!(state.update_count, update_count);
        assert_eq!(state.bid_volume_total, 13.0 * update_count as f64);
        assert_eq!(state.ask_volume_total, 7.0 * update_count as f64);
        assert_eq!(state.imbalance(), 0.3);

        for id in ids {
            let topic = get_book_deltas_topic(id);
            assert_eq!(msgbus::subscriber_count_deltas(topic), 0);
        }
    }
}

#[rstest]
fn test_imbalance_zero_when_no_volume() {
    let state = ImbalanceState::new();
    assert_eq!(state.imbalance(), 0.0);
}

#[rstest]
fn test_imbalance_positive_when_bid_dominated() {
    let mut state = ImbalanceState::new();
    state.bid_volume_total = 100.0;
    state.ask_volume_total = 20.0;

    let expected = (100.0 - 20.0) / (100.0 + 20.0);
    assert!((state.imbalance() - expected).abs() < 1e-10);
    assert!(state.imbalance() > 0.0);
}

#[rstest]
fn test_imbalance_negative_when_ask_dominated() {
    let mut state = ImbalanceState::new();
    state.bid_volume_total = 30.0;
    state.ask_volume_total = 70.0;

    let expected = (30.0 - 70.0) / (30.0 + 70.0);
    assert!((state.imbalance() - expected).abs() < 1e-10);
    assert!(state.imbalance() < 0.0);
}

#[rstest]
fn test_imbalance_zero_when_balanced() {
    let mut state = ImbalanceState::new();
    state.bid_volume_total = 50.0;
    state.ask_volume_total = 50.0;

    assert_eq!(state.imbalance(), 0.0);
}

#[rstest]
fn test_on_book_deltas_accumulates_bid_volume() {
    let id = instrument_a();
    let mut actor = BookImbalanceActor::new(vec![id], 0, None);

    let deltas = make_deltas(
        id,
        vec![
            make_delta(id, OrderSide::Buy, "2.50", "100", 1_000_000),
            make_delta(id, OrderSide::Buy, "2.48", "200", 1_000_000),
        ],
    );
    actor.on_book_deltas(&deltas).unwrap();

    let state = &actor.states()[&id];
    assert_eq!(state.update_count, 1);
    assert!((state.bid_volume_total - 300.0).abs() < 1e-10);
    assert_eq!(state.ask_volume_total, 0.0);
    assert_eq!(state.imbalance(), 1.0);
}

#[rstest]
fn test_on_book_deltas_accumulates_ask_volume() {
    let id = instrument_a();
    let mut actor = BookImbalanceActor::new(vec![id], 0, None);

    let deltas = make_deltas(
        id,
        vec![make_delta(id, OrderSide::Sell, "2.52", "150", 1_000_000)],
    );
    actor.on_book_deltas(&deltas).unwrap();

    let state = &actor.states()[&id];
    assert_eq!(state.ask_volume_total, 150.0);
    assert_eq!(state.imbalance(), -1.0);
}

#[rstest]
fn test_on_book_deltas_mixed_sides() {
    let id = instrument_a();
    let mut actor = BookImbalanceActor::new(vec![id], 0, None);

    let deltas = make_deltas(
        id,
        vec![
            make_delta(id, OrderSide::Buy, "2.50", "80", 1_000_000),
            make_delta(id, OrderSide::Sell, "2.52", "20", 1_000_000),
        ],
    );
    actor.on_book_deltas(&deltas).unwrap();

    let state = &actor.states()[&id];
    assert!((state.bid_volume_total - 80.0).abs() < 1e-10);
    assert!((state.ask_volume_total - 20.0).abs() < 1e-10);

    let expected = (80.0 - 20.0) / (80.0 + 20.0);
    assert!((state.imbalance() - expected).abs() < 1e-10);
}

#[rstest]
fn test_multiple_updates_accumulate() {
    let id = instrument_a();
    let mut actor = BookImbalanceActor::new(vec![id], 0, None);

    let deltas1 = make_deltas(
        id,
        vec![make_delta(id, OrderSide::Buy, "2.50", "100", 1_000_000)],
    );
    let deltas2 = make_deltas(
        id,
        vec![make_delta(id, OrderSide::Sell, "2.52", "60", 2_000_000)],
    );
    let deltas3 = make_deltas(
        id,
        vec![make_delta(id, OrderSide::Buy, "2.50", "40", 3_000_000)],
    );

    actor.on_book_deltas(&deltas1).unwrap();
    actor.on_book_deltas(&deltas2).unwrap();
    actor.on_book_deltas(&deltas3).unwrap();

    let state = &actor.states()[&id];
    assert_eq!(state.update_count, 3);
    assert!((state.bid_volume_total - 140.0).abs() < 1e-10);
    assert!((state.ask_volume_total - 60.0).abs() < 1e-10);
}

#[rstest]
fn test_multiple_instruments_tracked_independently() {
    let id_a = instrument_a();
    let id_b = instrument_b();
    let mut actor = BookImbalanceActor::new(vec![id_a, id_b], 0, None);

    let deltas_a = make_deltas(
        id_a,
        vec![make_delta(id_a, OrderSide::Buy, "2.50", "100", 1_000_000)],
    );
    let deltas_b = make_deltas(
        id_b,
        vec![make_delta(id_b, OrderSide::Sell, "3.00", "200", 1_000_000)],
    );

    actor.on_book_deltas(&deltas_a).unwrap();
    actor.on_book_deltas(&deltas_b).unwrap();

    assert_eq!(actor.states().len(), 2);

    let state_a = &actor.states()[&id_a];
    assert_eq!(state_a.imbalance(), 1.0);

    let state_b = &actor.states()[&id_b];
    assert_eq!(state_b.imbalance(), -1.0);
}

#[rstest]
fn test_unsubscribed_instrument_still_tracked() {
    let id_a = instrument_a();
    let id_b = instrument_b();
    // Actor configured for id_a only, but deltas for id_b still processed
    // (the engine routes data, the actor just handles what it receives)
    let mut actor = BookImbalanceActor::new(vec![id_a], 0, None);

    let deltas_b = make_deltas(
        id_b,
        vec![make_delta(id_b, OrderSide::Buy, "5.00", "50", 1_000_000)],
    );
    actor.on_book_deltas(&deltas_b).unwrap();

    assert!(actor.states().contains_key(&id_b));
}

#[rstest]
fn test_empty_deltas_batch_increments_count() {
    let id = instrument_a();
    let mut actor = BookImbalanceActor::new(vec![id], 0, None);

    // An empty deltas batch (clear action with no orders)
    let delta = OrderBookDelta::clear(id, 0, UnixNanos::from(1u64), UnixNanos::from(1u64));
    let deltas = make_deltas(id, vec![delta]);
    actor.on_book_deltas(&deltas).unwrap();

    let state = &actor.states()[&id];
    assert_eq!(state.update_count, 1);
    assert_eq!(state.bid_volume_total, 0.0);
    assert_eq!(state.ask_volume_total, 0.0);
    assert_eq!(state.imbalance(), 0.0);
}

#[rstest]
fn test_config_new_sets_defaults() {
    let ids = vec![instrument_a()];
    let config = BookImbalanceActorConfig::builder()
        .instrument_ids(ids.clone())
        .build();
    assert_eq!(config.instrument_ids, ids);
    assert_eq!(config.log_interval, 100);
    assert!(config.actor_id.is_none());
}

#[rstest]
fn test_config_builder_overrides() {
    let ids = vec![instrument_a()];
    let config = BookImbalanceActorConfig::builder()
        .instrument_ids(ids)
        .log_interval(50)
        .actor_id(ActorId::from("MY_ACTOR-001"))
        .build();
    assert_eq!(config.log_interval, 50);
    assert_eq!(config.actor_id, Some(ActorId::from("MY_ACTOR-001")));
}

#[rstest]
fn test_from_config_creates_actor() {
    let ids = vec![instrument_a(), instrument_b()];
    let config = BookImbalanceActorConfig::builder()
        .instrument_ids(ids)
        .log_interval(50)
        .actor_id(ActorId::from("MY_ACTOR-001"))
        .build();
    let actor = BookImbalanceActor::from_config(config);
    assert!(actor.states().is_empty());
    assert_eq!(actor.actor_id(), ActorId::from("MY_ACTOR-001"));
}

#[rstest]
fn test_on_reset_restores_constructor_state() {
    let id = instrument_a();
    let mut actor = BookImbalanceActor::new(vec![id], 50, None);
    register_actor(&mut actor);
    let deltas = make_deltas(
        id,
        vec![
            make_delta(id, OrderSide::Buy, "2.50", "100", 1_000_000),
            make_delta(id, OrderSide::Sell, "2.52", "40", 1_000_000),
        ],
    );
    actor.on_book_deltas(&deltas).unwrap();

    let state = &actor.states()[&id];
    assert_eq!(state.update_count, 1);
    assert_eq!(state.bid_volume_total, 100.0);
    assert_eq!(state.ask_volume_total, 40.0);

    actor.reset().unwrap();

    assert!(actor.states().is_empty());
    assert_eq!(actor.actor_id(), ActorId::from("BOOK_IMBALANCE-001"));
    assert!(actor.is_ready());
}

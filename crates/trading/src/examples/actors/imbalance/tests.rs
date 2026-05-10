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

use nautilus_common::actor::DataActor;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{BookOrder, OrderBookDelta, OrderBookDeltas},
    enums::{BookAction, OrderSide},
    identifiers::{ActorId, InstrumentId},
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

#[rstest]
fn test_new_actor_starts_with_empty_states() {
    let actor = BookImbalanceActor::new(vec![instrument_a()], 0, None);

    assert!(actor.states().is_empty());
    assert_eq!(actor.actor_id, ActorId::from("BOOK_IMBALANCE-001"));
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
    let config = BookImbalanceActorConfig::new(ids.clone());
    assert_eq!(config.instrument_ids, ids);
    assert_eq!(config.log_interval, 100);
    assert!(config.actor_id.is_none());
}

#[rstest]
fn test_config_builder_overrides() {
    let ids = vec![instrument_a()];
    let config = BookImbalanceActorConfig::new(ids)
        .with_log_interval(50)
        .with_actor_id(ActorId::from("MY_ACTOR-001"));
    assert_eq!(config.log_interval, 50);
    assert_eq!(config.actor_id, Some(ActorId::from("MY_ACTOR-001")));
}

#[rstest]
fn test_from_config_creates_actor() {
    let ids = vec![instrument_a(), instrument_b()];
    let config = BookImbalanceActorConfig::new(ids)
        .with_log_interval(50)
        .with_actor_id(ActorId::from("MY_ACTOR-001"));
    let actor = BookImbalanceActor::from_config(config);
    assert!(actor.states().is_empty());
    assert_eq!(actor.actor_id, ActorId::from("MY_ACTOR-001"));
}

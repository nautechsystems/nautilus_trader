// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Tests module for `Cache`.

#[cfg(feature = "defi")]
use std::sync::Arc;

use bytes::Bytes;
use nautilus_core::UnixNanos;
#[cfg(feature = "defi")]
use nautilus_model::defi::{AmmType, Dex, Pool, Token, chain::chains};
use nautilus_model::{
    accounts::AccountAny,
    data::{Bar, MarkPriceUpdate, QuoteTick, TradeTick},
    enums::{BookType, OmsType, OrderSide, OrderStatus, OrderType, PositionSide, PriceType},
    events::{OrderAccepted, OrderEventAny, OrderRejected, OrderSubmitted},
    identifiers::{AccountId, ClientOrderId, InstrumentId, PositionId, Symbol, Venue},
    instruments::{CurrencyPair, Instrument, InstrumentAny, SyntheticInstrument, stubs::*},
    orderbook::OrderBook,
    orders::{
        Order,
        builder::OrderTestBuilder,
        stubs::{TestOrderEventStubs, TestOrdersGenerator},
    },
    position::Position,
    types::{Currency, Price, Quantity},
};
use rstest::{fixture, rstest};

use crate::cache::Cache;

#[fixture]
fn cache() -> Cache {
    Cache::default()
}

#[rstest]
fn test_build_index_when_empty(mut cache: Cache) {
    cache.build_index();
}

#[rstest]
fn test_check_integrity_when_empty(mut cache: Cache) {
    let result = cache.check_integrity();
    assert!(result);
}

#[rstest]
fn test_check_residuals_when_empty(cache: Cache) {
    let result = cache.check_residuals();
    assert!(!result);
}

#[rstest]
fn test_clear_index_when_empty(mut cache: Cache) {
    cache.clear_index();
}

#[rstest]
fn test_reset_when_empty(mut cache: Cache) {
    cache.reset();
}

#[rstest]
fn test_dispose_when_empty(mut cache: Cache) {
    cache.dispose();
}

#[rstest]
fn test_flush_db_when_empty(mut cache: Cache) {
    cache.flush_db();
}

#[rstest]
fn test_cache_general_when_no_database(mut cache: Cache) {
    assert!(cache.cache_general().is_ok());
}

// -- EXECUTION -------------------------------------------------------------------------------

#[rstest]
#[tokio::test]
async fn test_cache_orders_when_no_database(mut cache: Cache) {
    assert!(cache.cache_orders().await.is_ok());
}

#[rstest]
fn test_order_when_empty(cache: Cache) {
    let client_order_id = ClientOrderId::default();
    let result = cache.order(&client_order_id);
    assert!(result.is_none());
}

#[rstest]
fn test_order_when_initialized(mut cache: Cache, audusd_sim: CurrencyPair) {
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(audusd_sim.id)
        .side(OrderSide::Buy)
        .price(Price::from("1.00000"))
        .quantity(Quantity::from(100_000))
        .build();

    let client_order_id = order.client_order_id();
    cache.add_order(order, None, None, false).unwrap();

    let order = cache.order(&client_order_id).unwrap();
    assert_eq!(cache.orders(None, None, None, None), vec![order]);
    assert!(cache.orders_open(None, None, None, None).is_empty());
    assert!(cache.orders_closed(None, None, None, None).is_empty());
    assert!(cache.orders_emulated(None, None, None, None).is_empty());
    assert!(cache.orders_inflight(None, None, None, None).is_empty());
    assert!(cache.order_exists(&order.client_order_id()));
    assert!(!cache.is_order_open(&order.client_order_id()));
    assert!(!cache.is_order_closed(&order.client_order_id()));
    assert!(!cache.is_order_emulated(&order.client_order_id()));
    assert!(!cache.is_order_inflight(&order.client_order_id()));
    assert!(!cache.is_order_pending_cancel_local(&order.client_order_id()));
    assert_eq!(cache.orders_open_count(None, None, None, None), 0);
    assert_eq!(cache.orders_closed_count(None, None, None, None), 0);
    assert_eq!(cache.orders_emulated_count(None, None, None, None), 0);
    assert_eq!(cache.orders_inflight_count(None, None, None, None), 0);
    assert_eq!(cache.orders_total_count(None, None, None, None), 1);
    assert_eq!(cache.venue_order_id(&order.client_order_id()), None);
}

#[rstest]
fn test_order_when_submitted(mut cache: Cache, audusd_sim: CurrencyPair) {
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(audusd_sim.id)
        .side(OrderSide::Buy)
        .price(Price::from("1.00000"))
        .quantity(Quantity::from(100_000))
        .build();

    let client_order_id = order.client_order_id();
    cache.add_order(order.clone(), None, None, false).unwrap();

    let submitted = OrderSubmitted::default();
    order.apply(OrderEventAny::Submitted(submitted)).unwrap();
    cache.update_order(&order).unwrap();

    // check the status change of the cached order
    let cached_order = cache.order(&client_order_id).unwrap();
    assert_eq!(cached_order.status(), OrderStatus::Submitted);

    let result = cache.order(&order.client_order_id()).unwrap();

    assert_eq!(order.status(), OrderStatus::Submitted);
    assert_eq!(result, &order);
    assert_eq!(cache.orders(None, None, None, None), vec![&order]);
    assert!(cache.orders_open(None, None, None, None).is_empty());
    assert!(cache.orders_closed(None, None, None, None).is_empty());
    assert!(cache.orders_emulated(None, None, None, None).is_empty());
    assert!(!cache.orders_inflight(None, None, None, None).is_empty());
    assert!(cache.order_exists(&order.client_order_id()));
    assert!(!cache.is_order_open(&order.client_order_id()));
    assert!(!cache.is_order_closed(&order.client_order_id()));
    assert!(!cache.is_order_emulated(&order.client_order_id()));
    assert!(cache.is_order_inflight(&order.client_order_id()));
    assert!(!cache.is_order_pending_cancel_local(&order.client_order_id()));
    assert_eq!(cache.orders_open_count(None, None, None, None), 0);
    assert_eq!(cache.orders_closed_count(None, None, None, None), 0);
    assert_eq!(cache.orders_emulated_count(None, None, None, None), 0);
    assert_eq!(cache.orders_inflight_count(None, None, None, None), 1);
    assert_eq!(cache.orders_total_count(None, None, None, None), 1);
    assert_eq!(cache.venue_order_id(&order.client_order_id()), None);
}

#[ignore = "Revisit on next pass"]
#[rstest]
fn test_order_when_rejected(mut cache: Cache, audusd_sim: CurrencyPair) {
    let mut order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(audusd_sim.id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from(100_000))
        .build();
    cache.add_order(order.clone(), None, None, false).unwrap();

    let submitted = OrderSubmitted::default();
    order.apply(OrderEventAny::Submitted(submitted)).unwrap();
    cache.update_order(&order).unwrap();

    let rejected = OrderRejected::default();
    order.apply(OrderEventAny::Rejected(rejected)).unwrap();
    cache.update_order(&order).unwrap();

    // check the status change of the cached order
    let cached_order = cache.order(&order.client_order_id()).unwrap();
    assert_eq!(cached_order.status(), OrderStatus::Rejected);

    let result = cache.order(&order.client_order_id()).unwrap();

    assert!(order.is_closed());
    assert_eq!(result, &order);
    assert_eq!(cache.orders(None, None, None, None), vec![&order]);
    assert!(cache.orders_open(None, None, None, None).is_empty());
    assert_eq!(cache.orders_closed(None, None, None, None), vec![&order]);
    assert!(cache.orders_emulated(None, None, None, None).is_empty());
    assert!(cache.orders_inflight(None, None, None, None).is_empty());
    assert!(cache.order_exists(&order.client_order_id()));
    assert!(!cache.is_order_open(&order.client_order_id()));
    assert!(cache.is_order_closed(&order.client_order_id()));
    assert!(!cache.is_order_emulated(&order.client_order_id()));
    assert!(!cache.is_order_inflight(&order.client_order_id()));
    assert!(!cache.is_order_pending_cancel_local(&order.client_order_id()));
    assert_eq!(cache.orders_open_count(None, None, None, None), 0);
    assert_eq!(cache.orders_closed_count(None, None, None, None), 1);
    assert_eq!(cache.orders_emulated_count(None, None, None, None), 0);
    assert_eq!(cache.orders_inflight_count(None, None, None, None), 0);
    assert_eq!(cache.orders_total_count(None, None, None, None), 1);
}

#[rstest]
fn test_order_when_accepted(mut cache: Cache, audusd_sim: CurrencyPair) {
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(audusd_sim.id)
        .side(OrderSide::Buy)
        .price(Price::from("1.00000"))
        .quantity(Quantity::from(100_000))
        .build();

    cache.add_order(order.clone(), None, None, false).unwrap();

    let submitted = OrderSubmitted::default();
    order.apply(OrderEventAny::Submitted(submitted)).unwrap();
    cache.update_order(&order).unwrap();

    let accepted = OrderAccepted::default();
    order.apply(OrderEventAny::Accepted(accepted)).unwrap();
    cache.update_order(&order).unwrap();

    let result = cache.order(&order.client_order_id()).unwrap();

    assert!(order.is_open());
    assert_eq!(result, &order);
    assert_eq!(cache.orders(None, None, None, None), vec![&order]);
    assert_eq!(cache.orders_open(None, None, None, None), vec![&order]);
    assert!(cache.orders_closed(None, None, None, None).is_empty());
    assert!(cache.orders_emulated(None, None, None, None).is_empty());
    assert!(cache.orders_inflight(None, None, None, None).is_empty());
    assert!(cache.order_exists(&order.client_order_id()));
    assert!(cache.is_order_open(&order.client_order_id()));
    assert!(!cache.is_order_closed(&order.client_order_id()));
    assert!(!cache.is_order_emulated(&order.client_order_id()));
    assert!(!cache.is_order_inflight(&order.client_order_id()));
    assert!(!cache.is_order_pending_cancel_local(&order.client_order_id()));
    assert_eq!(cache.orders_open_count(None, None, None, None), 1);
    assert_eq!(cache.orders_closed_count(None, None, None, None), 0);
    assert_eq!(cache.orders_emulated_count(None, None, None, None), 0);
    assert_eq!(cache.orders_inflight_count(None, None, None, None), 0);
    assert_eq!(cache.orders_total_count(None, None, None, None), 1);
    assert_eq!(
        cache.client_order_id(&order.venue_order_id().unwrap()),
        Some(&order.client_order_id())
    );
    assert_eq!(
        cache.venue_order_id(&order.client_order_id()),
        Some(&order.venue_order_id().unwrap())
    );
}

#[rstest]
fn test_client_order_ids_filtering(mut cache: Cache) {
    // Build a small deterministic universe: 2 venues × 3 instruments × 2 orders
    let venue_a = Venue::from("VENUE-A");
    let _venue_b = Venue::from("VENUE-B");

    let mut generator = TestOrdersGenerator::new(OrderType::Limit);
    generator.add_venue_and_total_instruments(venue_a, 3);
    generator.add_venue_and_total_instruments(_venue_b, 3);
    generator.set_orders_per_instrument(2);

    let orders = generator.build();

    let _instrument_a0 = InstrumentId::from("SYMBOL-0.VENUE-A");

    // Sanity-check the generated volume: 2 × 3 × 2 = 12
    assert_eq!(orders.len(), 12);

    // Load into cache so indices are built on the fly
    for order in &orders {
        cache.add_order(order.clone(), None, None, false).unwrap();
    }

    // No filters – expect all orders
    assert_eq!(cache.client_order_ids(None, None, None).len(), orders.len());

    // Venue only
    let expected_venue_a = orders
        .iter()
        .filter(|o| o.instrument_id().venue == venue_a)
        .count();
    assert_eq!(
        cache.client_order_ids(Some(&venue_a), None, None).len(),
        expected_venue_a
    );

    // Venue + instrument
    let instrument_a0 = InstrumentId::from("SYMBOL-0.VENUE-A");
    assert_eq!(
        cache
            .client_order_ids(Some(&venue_a), Some(&instrument_a0), None)
            .len(),
        orders
            .iter()
            .filter(|o| o.instrument_id() == instrument_a0)
            .count()
    );
}

#[rstest]
fn test_position_ids_filtering(mut cache: Cache) {
    let venue_a = Venue::from("VENUE-A");
    let _venue_b = Venue::from("VENUE-B");

    fn make_pair(id_str: &str) -> CurrencyPair {
        CurrencyPair::new(
            InstrumentId::from(id_str),
            Symbol::from(id_str),
            Currency::USD(),
            Currency::EUR(),
            2,
            4,
            Price::from("0.01"),
            Quantity::from("0.0001"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        )
    }

    // Build two open positions and one closed position across venues
    let instr_a0 = make_pair("PAIR-0.VENUE-A");
    let instr_b0 = make_pair("PAIR-0.VENUE-B");

    let base_order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instr_a0.id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1"))
        .build();

    let fill_a_event = TestOrderEventStubs::filled(
        &base_order,
        &InstrumentAny::CurrencyPair(instr_a0),
        None,
        Some(PositionId::new("POS-A")),
        None,
        None,
        None,
        None,
        None,
        None,
    );
    let fill_a = match fill_a_event {
        OrderEventAny::Filled(f) => f,
        _ => unreachable!(),
    };
    let pos_a = Position::new(&InstrumentAny::CurrencyPair(instr_a0), fill_a);

    // Second open position on venue B
    let order_b = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instr_b0.id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1"))
        .build();

    let fill_b_event = TestOrderEventStubs::filled(
        &order_b,
        &InstrumentAny::CurrencyPair(instr_b0),
        None,
        Some(PositionId::new("POS-B")),
        None,
        None,
        None,
        None,
        None,
        None,
    );
    let fill_b = match fill_b_event {
        OrderEventAny::Filled(f) => f,
        _ => unreachable!(),
    };
    let pos_b = Position::new(&InstrumentAny::CurrencyPair(instr_b0), fill_b);

    // Closed position on venue A (side Flat + ts_closed)
    let mut pos_closed = pos_a.clone();
    pos_closed.id = PositionId::new("POS-C");
    pos_closed.side = PositionSide::Flat;
    pos_closed.ts_closed = Some(UnixNanos::from(1));

    // Insert into cache
    cache.add_position(pos_a.clone(), OmsType::Netting).unwrap();
    cache.add_position(pos_b.clone(), OmsType::Netting).unwrap();
    cache
        .add_position(pos_closed.clone(), OmsType::Netting)
        .unwrap();

    // Assertions
    assert_eq!(cache.position_ids(None, None, None).len(), 3);

    // Venue filter
    assert_eq!(cache.position_ids(Some(&venue_a), None, None).len(), 2);

    // Venue + instrument filter
    assert_eq!(
        cache
            .position_ids(Some(&venue_a), Some(&instr_a0.id), None)
            .len(),
        2 // open + closed on venue A instrument
    );

    // Open / closed separation
    assert!(
        cache
            .position_open_ids(None, None, None)
            .contains(&pos_a.id)
    );
}

#[ignore = "Revisit on next pass"]
#[rstest]
fn test_order_when_filled(mut cache: Cache, audusd_sim: CurrencyPair) {
    let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);
    let mut order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(audusd_sim.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from(100_000))
        .build();
    cache.add_order(order.clone(), None, None, false).unwrap();

    let submitted = OrderSubmitted::default();
    order.apply(OrderEventAny::Submitted(submitted)).unwrap();
    cache.update_order(&order).unwrap();

    let accepted = OrderAccepted::default();
    order.apply(OrderEventAny::Accepted(accepted)).unwrap();
    cache.update_order(&order).unwrap();

    let filled = TestOrderEventStubs::filled(
        &order,
        &audusd_sim,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    );
    order.apply(filled).unwrap();
    cache.update_order(&order).unwrap();

    let result = cache.order(&order.client_order_id()).unwrap();

    assert!(order.is_closed());
    assert_eq!(result, &order);
    assert_eq!(cache.orders(None, None, None, None), vec![&order]);
    assert_eq!(cache.orders_closed(None, None, None, None), vec![&order]);
    assert!(cache.orders_open(None, None, None, None).is_empty());
    assert!(cache.orders_emulated(None, None, None, None).is_empty());
    assert!(cache.orders_inflight(None, None, None, None).is_empty());
    assert!(cache.order_exists(&order.client_order_id()));
    assert!(!cache.is_order_open(&order.client_order_id()));
    assert!(cache.is_order_closed(&order.client_order_id()));
    assert!(!cache.is_order_emulated(&order.client_order_id()));
    assert!(!cache.is_order_inflight(&order.client_order_id()));
    assert!(!cache.is_order_pending_cancel_local(&order.client_order_id()));
    assert_eq!(cache.orders_open_count(None, None, None, None), 0);
    assert_eq!(cache.orders_closed_count(None, None, None, None), 1);
    assert_eq!(cache.orders_emulated_count(None, None, None, None), 0);
    assert_eq!(cache.orders_inflight_count(None, None, None, None), 0);
    assert_eq!(cache.orders_total_count(None, None, None, None), 1);
    assert_eq!(
        cache.client_order_id(&order.venue_order_id().unwrap()),
        Some(&order.client_order_id())
    );
    assert_eq!(
        cache.venue_order_id(&order.client_order_id()),
        Some(&order.venue_order_id().unwrap())
    );
}

#[rstest]
fn test_get_general_when_empty(cache: Cache) {
    let result = cache.get("A").unwrap();
    assert!(result.is_none());
}

#[rstest]
fn test_add_general_when_value(mut cache: Cache) {
    let key = "A";
    let value = Bytes::from_static(&[0_u8]);
    cache.add(key, value.clone()).unwrap();
    let result = cache.get(key).unwrap();
    assert_eq!(result, Some(&value));
}

#[rstest]
fn test_orders_for_position(mut cache: Cache, audusd_sim: CurrencyPair) {
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(audusd_sim.id)
        .side(OrderSide::Buy)
        .price(Price::from("1.00000"))
        .quantity(Quantity::from(100_000))
        .build();

    let position_id = PositionId::default();
    cache
        .add_order(order.clone(), Some(position_id), None, false)
        .unwrap();
    let result = cache.order(&order.client_order_id()).unwrap();
    assert_eq!(result, &order);
    assert_eq!(cache.orders_for_position(&position_id), vec![&order]);
}

#[rstest]
fn test_correct_order_indexing(mut cache: Cache) {
    let binance = Venue::from("BINANCE");
    let bybit = Venue::from("BYBIT");
    let mut orders_generator = TestOrdersGenerator::new(OrderType::Limit);
    orders_generator.add_venue_and_total_instruments(bybit, 10);
    orders_generator.add_venue_and_total_instruments(binance, 10);
    orders_generator.set_orders_per_instrument(2);
    let orders = orders_generator.build();
    // There will be 2 Venues * 10 Instruments * 2 Orders = 40 Orders
    assert_eq!(orders.len(), 40);
    for order in orders {
        cache.add_order(order, None, None, false).unwrap();
    }
    assert_eq!(cache.orders(None, None, None, None).len(), 40);
    assert_eq!(cache.orders(Some(&bybit), None, None, None).len(), 20);
    assert_eq!(cache.orders(Some(&binance), None, None, None).len(), 20);
    assert_eq!(
        cache
            .orders(
                Some(&bybit),
                Some(&InstrumentId::from("SYMBOL-0.BYBIT")),
                None,
                None
            )
            .len(),
        2
    );
    assert_eq!(
        cache
            .orders(
                Some(&binance),
                Some(&InstrumentId::from("SYMBOL-0.BINANCE")),
                None,
                None
            )
            .len(),
        2
    );
}

#[rstest]
#[tokio::test]
async fn test_cache_positions_when_no_database(mut cache: Cache) {
    assert!(cache.cache_positions().await.is_ok());
}

#[rstest]
fn test_position_when_empty(cache: Cache) {
    let position_id = PositionId::from("1");
    let result = cache.position(&position_id);
    assert!(result.is_none());
    assert!(!cache.position_exists(&position_id));
}

#[rstest]
fn test_position_when_some(mut cache: Cache, audusd_sim: CurrencyPair) {
    let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(audusd_sim.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from(100_000))
        .build();
    let filled = TestOrderEventStubs::filled(
        &order,
        &audusd_sim,
        None,
        Some(PositionId::new("P-123456")),
        None,
        None,
        None,
        None,
        None,
        None,
    );
    let position = Position::new(&audusd_sim, filled.into());
    cache
        .add_position(position.clone(), OmsType::Netting)
        .unwrap();

    let result = cache.position(&position.id);
    assert_eq!(result, Some(&position));
    assert!(cache.position_exists(&position.id));
    assert_eq!(
        cache.position_id(&order.client_order_id()),
        Some(&position.id)
    );
    assert_eq!(
        cache.positions_open(None, None, None, None),
        vec![&position]
    );
    assert_eq!(
        cache.positions_closed(None, None, None, None),
        Vec::<&Position>::new()
    );
    assert_eq!(cache.positions_open_count(None, None, None, None), 1);
    assert_eq!(cache.positions_closed_count(None, None, None, None), 0);
}

// -- DATA ------------------------------------------------------------------------------------

#[rstest]
#[tokio::test]
async fn test_cache_currencies_when_no_database(mut cache: Cache) {
    assert!(cache.cache_currencies().await.is_ok());
}

#[rstest]
#[tokio::test]
async fn test_cache_instruments_when_no_database(mut cache: Cache) {
    assert!(cache.cache_instruments().await.is_ok());
}

#[rstest]
fn test_instrument_when_empty(cache: Cache, audusd_sim: CurrencyPair) {
    let result = cache.instrument(&audusd_sim.id);
    assert!(result.is_none());
}

#[rstest]
fn test_instrument_when_some(mut cache: Cache, audusd_sim: CurrencyPair) {
    cache
        .add_instrument(InstrumentAny::CurrencyPair(audusd_sim))
        .unwrap();

    let result = cache.instrument(&audusd_sim.id);
    assert_eq!(result, Some(&InstrumentAny::CurrencyPair(audusd_sim)));
}

#[rstest]
fn test_instruments_when_empty(cache: Cache) {
    let esz1 = futures_contract_es(None, None);
    let result = cache.instruments(&esz1.id.venue, None);
    assert!(result.is_empty());
}

#[rstest]
fn test_instruments_when_some(mut cache: Cache) {
    let esz1 = futures_contract_es(None, None);
    cache
        .add_instrument(InstrumentAny::FuturesContract(esz1))
        .unwrap();

    let result1 = cache.instruments(&esz1.id.venue, None);
    let result2 = cache.instruments(&esz1.id.venue, Some(&esz1.underlying));
    assert_eq!(result1, vec![&InstrumentAny::FuturesContract(esz1)]);
    assert_eq!(result2, vec![&InstrumentAny::FuturesContract(esz1)]);
}

#[rstest]
#[tokio::test]
async fn test_cache_synthetics_when_no_database(mut cache: Cache) {
    assert!(cache.cache_synthetics().await.is_ok());
}

#[rstest]
fn test_synthetic_when_empty(cache: Cache) {
    let synth = SyntheticInstrument::default();
    let result = cache.synthetic(&synth.id);
    assert!(result.is_none());
}

#[rstest]
fn test_synthetic_when_some(mut cache: Cache) {
    let synth = SyntheticInstrument::default();
    cache.add_synthetic(synth.clone()).unwrap();
    let result = cache.synthetic(&synth.id);
    assert_eq!(result, Some(&synth));
}

#[rstest]
fn test_order_book_when_empty(cache: Cache, audusd_sim: CurrencyPair) {
    let result = cache.order_book(&audusd_sim.id);
    assert!(result.is_none());
}

#[rstest]
fn test_order_book_when_some(mut cache: Cache, audusd_sim: CurrencyPair) {
    let book = OrderBook::new(audusd_sim.id, BookType::L2_MBP);
    cache.add_order_book(book.clone()).unwrap();
    let result = cache.order_book(&audusd_sim.id);
    assert_eq!(result, Some(&book));
}

#[rstest]
fn test_order_book_mut_when_empty(mut cache: Cache, audusd_sim: CurrencyPair) {
    let result = cache.order_book_mut(&audusd_sim.id);
    assert!(result.is_none());
}

#[rstest]
fn test_order_book_mut_when_some(mut cache: Cache, audusd_sim: CurrencyPair) {
    let mut book = OrderBook::new(audusd_sim.id, BookType::L2_MBP);
    cache.add_order_book(book.clone()).unwrap();
    let result = cache.order_book_mut(&audusd_sim.id);
    assert_eq!(result, Some(&mut book));
}

#[cfg(feature = "defi")]
#[fixture]
fn test_pool() -> Pool {
    let chain = Arc::new(chains::ETHEREUM.clone());
    let dex = Dex::new(
        chains::ETHEREUM.clone(),
        "UniswapV3",
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        AmmType::CLAMM,
        "PoolCreated(address,address,uint24,int24,address)",
        "Swap(address,address,int256,int256,uint160,uint128,int24)",
        "Mint(address,address,int24,int24,uint128,uint256,uint256)",
        "Burn(address,int24,int24,uint128,uint256,uint256)",
    );

    let token0 = Token::new(
        chain.clone(),
        "0xA0b86a33E6441b936662bb6B5d1F8Fb0E2b57A5D"
            .parse()
            .unwrap(),
        "Wrapped Ether".to_string(),
        "WETH".to_string(),
        18,
    );

    let token1 = Token::new(
        chain.clone(),
        "0xdAC17F958D2ee523a2206206994597C13D831ec7"
            .parse()
            .unwrap(),
        "Tether USD".to_string(),
        "USDT".to_string(),
        6,
    );

    Pool::new(
        chain,
        dex,
        "0x11b815efB8f581194ae79006d24E0d814B7697F6"
            .parse()
            .unwrap(),
        12345678,
        token0,
        token1,
        3000,
        60,
        UnixNanos::from(1_234_567_890_000_000_000u64),
    )
}

#[cfg(feature = "defi")]
#[rstest]
fn test_pool_when_empty(cache: Cache, test_pool: Pool) {
    let instrument_id = test_pool.instrument_id;
    let result = cache.pool(&instrument_id);
    assert!(result.is_none());
}

#[cfg(feature = "defi")]
#[rstest]
fn test_pool_when_some(mut cache: Cache, test_pool: Pool) {
    let instrument_id = test_pool.instrument_id;
    cache.add_pool(test_pool.clone()).unwrap();
    let result = cache.pool(&instrument_id);
    assert_eq!(result, Some(&test_pool));
}

#[cfg(feature = "defi")]
#[rstest]
fn test_pool_mut_when_empty(mut cache: Cache, test_pool: Pool) {
    let instrument_id = test_pool.instrument_id;
    let result = cache.pool_mut(&instrument_id);
    assert!(result.is_none());
}

#[cfg(feature = "defi")]
#[rstest]
fn test_pool_mut_when_some(mut cache: Cache, test_pool: Pool) {
    let instrument_id = test_pool.instrument_id;
    cache.add_pool(test_pool.clone()).unwrap();
    let result = cache.pool_mut(&instrument_id);

    assert!(result.is_some());
    if let Some(pool_ref) = result {
        assert_eq!(pool_ref.fee, 3000);
    }
}

#[cfg(feature = "defi")]
#[rstest]
fn test_add_pool(mut cache: Cache, test_pool: Pool) {
    let instrument_id = test_pool.instrument_id;

    cache.add_pool(test_pool.clone()).unwrap();

    let cached_pool = cache.pool(&instrument_id);
    assert!(cached_pool.is_some());
    assert_eq!(cached_pool.unwrap(), &test_pool);
}

#[rstest]
#[case(PriceType::Bid)]
#[case(PriceType::Ask)]
#[case(PriceType::Mid)]
#[case(PriceType::Last)]
#[case(PriceType::Mark)]
fn test_price_when_empty(cache: Cache, audusd_sim: CurrencyPair, #[case] price_type: PriceType) {
    let result = cache.price(&audusd_sim.id, price_type);
    assert!(result.is_none());
}

#[rstest]
fn test_price_when_some(mut cache: Cache, audusd_sim: CurrencyPair) {
    let mark_price = MarkPriceUpdate::new(
        audusd_sim.id,
        Price::from("1.00000"),
        UnixNanos::from(5),
        UnixNanos::from(10),
    );
    cache.add_mark_price(mark_price).unwrap();
    let result = cache.price(&audusd_sim.id, PriceType::Mark);
    assert_eq!(result, Some(mark_price.value));
}

#[rstest]
fn test_quote_tick_when_empty(cache: Cache, audusd_sim: CurrencyPair) {
    let result = cache.quote(&audusd_sim.id);
    assert!(result.is_none());
}

#[rstest]
fn test_quote_tick_when_some(mut cache: Cache) {
    let quote = QuoteTick::default();
    cache.add_quote(quote).unwrap();
    let result = cache.quote(&quote.instrument_id);
    assert_eq!(result, Some(&quote));
}

#[rstest]
fn test_quote_ticks_when_empty(cache: Cache, audusd_sim: CurrencyPair) {
    let result = cache.quotes(&audusd_sim.id);
    assert!(result.is_none());
}

#[rstest]
fn test_quote_ticks_when_some(mut cache: Cache) {
    let quotes = vec![
        QuoteTick::default(),
        QuoteTick::default(),
        QuoteTick::default(),
    ];
    cache.add_quotes(&quotes).unwrap();
    let result = cache.quotes(&quotes[0].instrument_id);
    assert_eq!(result, Some(quotes));
}

#[rstest]
fn test_trade_tick_when_empty(cache: Cache, audusd_sim: CurrencyPair) {
    let result = cache.trade(&audusd_sim.id);
    assert!(result.is_none());
}

#[rstest]
fn test_trade_tick_when_some(mut cache: Cache) {
    let trade = TradeTick::default();
    cache.add_trade(trade).unwrap();
    let result = cache.trade(&trade.instrument_id);
    assert_eq!(result, Some(&trade));
}

#[rstest]
fn test_trade_ticks_when_empty(cache: Cache, audusd_sim: CurrencyPair) {
    let result = cache.trades(&audusd_sim.id);
    assert!(result.is_none());
}

#[rstest]
fn test_trade_ticks_when_some(mut cache: Cache) {
    let trades = vec![
        TradeTick::default(),
        TradeTick::default(),
        TradeTick::default(),
    ];
    cache.add_trades(&trades).unwrap();
    let result = cache.trades(&trades[0].instrument_id);
    assert_eq!(result, Some(trades));
}

#[rstest]
fn test_mark_price_when_empty(cache: Cache, audusd_sim: CurrencyPair) {
    let result = cache.mark_price(&audusd_sim.id);
    assert!(result.is_none());
}

#[rstest]
fn test_mark_prices_when_empty(cache: Cache, audusd_sim: CurrencyPair) {
    let result = cache.mark_prices(&audusd_sim.id);
    assert!(result.is_none());
}

#[rstest]
fn test_index_price_when_empty(cache: Cache, audusd_sim: CurrencyPair) {
    let result = cache.index_price(&audusd_sim.id);
    assert!(result.is_none());
}

#[rstest]
fn test_index_prices_when_empty(cache: Cache, audusd_sim: CurrencyPair) {
    let result = cache.index_prices(&audusd_sim.id);
    assert!(result.is_none());
}

#[rstest]
fn test_bar_when_empty(cache: Cache) {
    let bar = Bar::default();
    let result = cache.bar(&bar.bar_type);
    assert!(result.is_none());
}

#[rstest]
fn test_bar_when_some(mut cache: Cache) {
    let bar = Bar::default();
    cache.add_bar(bar).unwrap();
    let result = cache.bar(&bar.bar_type);
    assert_eq!(result, Some(&bar));
}

#[rstest]
fn test_bars_when_empty(cache: Cache) {
    let bar = Bar::default();
    let result = cache.bars(&bar.bar_type);
    assert!(result.is_none());
}

#[rstest]
fn test_bars_when_some(mut cache: Cache) {
    let bars = vec![Bar::default(), Bar::default(), Bar::default()];
    cache.add_bars(&bars).unwrap();
    let result = cache.bars(&bars[0].bar_type);
    assert_eq!(result, Some(bars));
}

// -- ACCOUNT ---------------------------------------------------------------------------------

#[rstest]
#[tokio::test]
async fn test_cache_accounts_when_no_database(mut cache: Cache) {
    assert!(cache.cache_accounts().await.is_ok());
}

#[rstest]
fn test_cache_add_account(mut cache: Cache) {
    let account = AccountAny::default();
    cache.add_account(account.clone()).unwrap();
    let result = cache.account(&account.id());
    assert!(result.is_some());
    assert_eq!(*result.unwrap(), account);
}

#[rstest]
fn test_cache_accounts_when_no_accounts_returns_empty(cache: Cache) {
    let result = cache.accounts(&AccountId::default());
    assert!(result.is_empty());
}

#[rstest]
fn test_cache_account_for_venue_returns_empty(cache: Cache) {
    let venue = Venue::default();
    let result = cache.account_for_venue(&venue);
    assert!(result.is_none());
}

#[rstest]
fn test_cache_account_for_venue_return_correct(mut cache: Cache) {
    let account = AccountAny::default();
    let venue = account.last_event().unwrap().account_id.get_issuer();
    cache.add_account(account.clone()).unwrap();
    let result = cache.account_for_venue(&venue);
    assert!(result.is_some());
    assert_eq!(*result.unwrap(), account);
}

#[rstest]
fn test_get_mark_xrate_returns_none(cache: Cache) {
    // When no mark xrate is set for (USD, EUR), it should return None
    assert!(
        cache
            .get_mark_xrate(Currency::USD(), Currency::EUR())
            .is_none()
    );
}

#[rstest]
fn test_set_and_get_mark_xrate(mut cache: Cache) {
    // Set a mark xrate for (USD, EUR) and check both forward and inverse rates
    let xrate = 1.25;
    cache.set_mark_xrate(Currency::USD(), Currency::EUR(), xrate);
    assert_eq!(
        cache.get_mark_xrate(Currency::USD(), Currency::EUR()),
        Some(xrate)
    );
    assert_eq!(
        cache.get_mark_xrate(Currency::EUR(), Currency::USD()),
        Some(1.0 / xrate)
    );
}

#[rstest]
fn test_clear_mark_xrate(mut cache: Cache) {
    // Set a rate and then clear the forward key
    let xrate = 1.25;
    cache.set_mark_xrate(Currency::USD(), Currency::EUR(), xrate);
    assert!(
        cache
            .get_mark_xrate(Currency::USD(), Currency::EUR())
            .is_some()
    );
    cache.clear_mark_xrate(Currency::USD(), Currency::EUR());
    assert!(
        cache
            .get_mark_xrate(Currency::USD(), Currency::EUR())
            .is_none()
    );
    assert_eq!(
        cache.get_mark_xrate(Currency::EUR(), Currency::USD()),
        Some(1.0 / xrate)
    );
}

#[rstest]
fn test_clear_mark_xrates(mut cache: Cache) {
    // Set two mark xrates and then clear them all
    cache.set_mark_xrate(Currency::USD(), Currency::EUR(), 1.25);
    cache.set_mark_xrate(Currency::AUD(), Currency::USD(), 0.75);
    cache.clear_mark_xrates();
    assert!(
        cache
            .get_mark_xrate(Currency::USD(), Currency::EUR())
            .is_none()
    );
    assert!(
        cache
            .get_mark_xrate(Currency::EUR(), Currency::USD())
            .is_none()
    );
    assert!(
        cache
            .get_mark_xrate(Currency::AUD(), Currency::USD())
            .is_none()
    );
    assert!(
        cache
            .get_mark_xrate(Currency::USD(), Currency::AUD())
            .is_none()
    );
}

#[rstest]
#[should_panic(expected = "xrate was zero")]
fn test_set_mark_xrate_panics_on_zero(mut cache: Cache) {
    // Setting a mark xrate of zero should panic
    cache.set_mark_xrate(Currency::USD(), Currency::EUR(), 0.0);
}

#[rstest]
fn test_purge_order() {
    let mut cache = Cache::default();
    let audusd_sim = audusd_sim();
    let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);

    // Create an order and fill to generate a position
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(audusd_sim.id())
        .side(OrderSide::Buy)
        .price(Price::from("1.00000"))
        .quantity(Quantity::from(100_000))
        .build();

    let client_order_id = order.client_order_id();

    let filled = TestOrderEventStubs::filled(
        &order,
        &audusd_sim,
        None,
        Some(PositionId::new("P-123456")),
        Some(Price::from("1.00001")),
        None,
        None,
        None,
        None,
        None,
    );

    cache.add_order(order, None, None, false).unwrap();

    let position = Position::new(&audusd_sim, filled.into());
    let position_id = position.id;
    cache.add_position(position, OmsType::Netting).unwrap();

    // Verify the order exists
    assert!(cache.order_exists(&client_order_id));
    assert_eq!(cache.orders_total_count(None, None, None, None), 1);

    // Purge the order
    cache.purge_order(client_order_id);

    // Verify the order is gone
    assert!(!cache.order_exists(&client_order_id));
    assert_eq!(cache.orders_total_count(None, None, None, None), 0);
    // Verify associated order events were purged from the position
    assert_eq!(cache.position(&position_id).unwrap().event_count(), 0);
}

#[rstest]
fn test_purge_position() {
    let mut cache = Cache::default();
    let audusd_sim = audusd_sim();
    let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);

    // Create an order and fill to generate a position
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(audusd_sim.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from(100_000))
        .build();

    let filled = TestOrderEventStubs::filled(
        &order,
        &audusd_sim,
        None,
        Some(PositionId::new("P-123456")),
        Some(Price::from("1.00001")),
        None,
        None,
        None,
        None,
        None,
    );

    let position = Position::new(&audusd_sim, filled.into());
    let position_id = position.id;

    // Add position to cache
    cache.add_position(position, OmsType::Netting).unwrap();

    // Verify the position exists
    assert!(cache.position_exists(&position_id));
    assert_eq!(cache.positions_total_count(None, None, None, None), 1);

    // Purge the position
    cache.purge_position(position_id);

    // Verify the position is gone
    assert!(!cache.position_exists(&position_id));
    assert_eq!(cache.positions_total_count(None, None, None, None), 0);
}

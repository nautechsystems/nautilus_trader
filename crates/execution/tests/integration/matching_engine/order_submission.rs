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

use super::*;

#[rstest]
fn test_process_order_rejects_reduce_only_when_support_is_disabled() {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let cache = Rc::new(RefCell::new(Cache::default()));

    let mut engine = OrderMatchingEngine::new(
        instrument.clone(),
        1,
        FillModelHandle::default(),
        FeeModelAny::MakerTaker(MakerTakerFeeModel::zero()).into(),
        BookType::L1_MBP,
        OmsType::Netting,
        AccountType::Margin,
        Rc::new(RefCell::new(VirtualClock::new())),
        cache.clone(),
        OrderMatchingEngineConfig::builder()
            .use_reduce_only(false)
            .build(),
    );
    let events = Rc::new(RefCell::new(Vec::new()));
    let events_handler = Rc::clone(&events);
    engine.set_event_handler(Rc::new(move |event| {
        events_handler.borrow_mut().push(event);
    }));

    let mut order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from("1.000"))
        .reduce_only(true)
        .submit(true)
        .build();

    cache_order(&cache, &order);
    engine.process_order(&mut order, AccountId::from("ACCOUNT-001"));

    let events = events.borrow();
    assert_eq!(events.len(), 1);

    let OrderEventAny::Rejected(rejected) = &events[0] else {
        panic!("Expected OrderRejected, was {:?}", events[0]);
    };

    assert_eq!(
        rejected.reason,
        "Reduce-only orders are not supported by this matching engine"
    );
}

#[rstest]
#[case(OrderType::Market)]
#[case(OrderType::MarketToLimit)]
fn test_market_collision_probes_and_fills_with_default_ack_config(#[case] order_type: OrderType) {
    let (mut engine, cache, venue_order_id) = collision_engine();
    assert!(!engine.config.use_market_order_acks);

    let quote = QuoteTick::new(
        engine.instrument.id(),
        Price::from("1499.00"),
        Price::from("1500.00"),
        Quantity::from("10.000"),
        Quantity::from("10.000"),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    engine.process_quote_tick(&quote);
    let events = Rc::new(RefCell::new(Vec::new()));
    let events_handler = Rc::clone(&events);
    engine.set_event_handler(Rc::new(move |event| {
        events_handler.borrow_mut().push(event);
    }));

    let mut order = OrderTestBuilder::new(order_type)
        .instrument_id(engine.instrument.id())
        .client_order_id(ClientOrderId::from("O-CLAIMANT"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .submit(true)
        .build();

    cache_order(&cache, &order);
    engine.process_order(&mut order, AccountId::from("ACCOUNT-001"));

    assert!(
        !events
            .borrow()
            .iter()
            .any(|event| matches!(event, OrderEventAny::Rejected(_)))
    );
    assert!(
        events
            .borrow()
            .iter()
            .any(|event| matches!(event, OrderEventAny::Filled(_)))
    );
    assert!(cache.borrow().order_exists(&order.client_order_id()));
    assert_eq!(
        cache.borrow().client_order_id(&venue_order_id),
        Some(&ClientOrderId::from("O-OWNER"))
    );
    assert_eq!(
        cache.borrow().venue_order_id(&order.client_order_id()),
        Some(&VenueOrderId::from(format!("{}-1-2", engine.venue)))
    );
}

fn collision_engine() -> (OrderMatchingEngine, Rc<RefCell<Cache>>, VenueOrderId) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let cache = Rc::new(RefCell::new(Cache::default()));
    let venue_order_id = VenueOrderId::from(format!("{}-1-1", instrument.id().venue));
    cache
        .borrow_mut()
        .add_venue_order_id(&ClientOrderId::from("O-OWNER"), &venue_order_id, false)
        .unwrap();

    let engine = OrderMatchingEngine::new(
        instrument,
        1,
        FillModelHandle::default(),
        FeeModelAny::MakerTaker(MakerTakerFeeModel::zero()).into(),
        BookType::L1_MBP,
        OmsType::Netting,
        AccountType::Margin,
        Rc::new(RefCell::new(VirtualClock::new())),
        Rc::clone(&cache),
        Default::default(),
    );

    (engine, cache, venue_order_id)
}

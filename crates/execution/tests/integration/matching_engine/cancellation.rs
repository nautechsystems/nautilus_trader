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
fn test_process_cancel_all_includes_submitted_orders_for_selected_account() {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let instrument_id = instrument.id();
    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock = Rc::new(RefCell::new(VirtualClock::new()));

    let mut engine = OrderMatchingEngine::new(
        instrument,
        1,
        FillModelHandle::default(),
        FeeModelAny::MakerTaker(MakerTakerFeeModel::zero()).into(),
        BookType::L1_MBP,
        OmsType::Netting,
        AccountType::Margin,
        clock,
        Rc::clone(&cache),
        Default::default(),
    );
    let selected_account = AccountId::from("ACCOUNT-001");
    let other_account = AccountId::from("ACCOUNT-002");
    let selected_strategy = StrategyId::from("STRATEGY-001");
    let other_strategy = StrategyId::from("STRATEGY-002");
    let selected_order = OrderTestBuilder::new(OrderType::Limit)
        .strategy_id(selected_strategy)
        .instrument_id(instrument_id)
        .client_order_id(ClientOrderId::from("O-SUBMITTED-SELECTED"))
        .side(OrderSide::Buy)
        .price(Price::from("1400.00"))
        .quantity(Quantity::from("1.000"))
        .build();
    let other_order = OrderTestBuilder::new(OrderType::Limit)
        .strategy_id(other_strategy)
        .instrument_id(instrument_id)
        .client_order_id(ClientOrderId::from("O-SUBMITTED-OTHER"))
        .side(OrderSide::Buy)
        .price(Price::from("1300.00"))
        .quantity(Quantity::from("1.000"))
        .build();
    {
        let mut cache = cache.borrow_mut();
        cache
            .add_order(selected_order.clone(), None, None, false)
            .unwrap();
        cache
            .add_order(other_order.clone(), None, None, false)
            .unwrap();
        cache
            .update_order(&TestOrderEventStubs::submitted(
                &selected_order,
                selected_account,
            ))
            .unwrap();
        cache
            .update_order(&TestOrderEventStubs::submitted(&other_order, other_account))
            .unwrap();
    }

    let events = Rc::new(RefCell::new(Vec::new()));
    let events_handler = Rc::clone(&events);
    let event_cache = Rc::clone(&cache);
    engine.set_event_handler(Rc::new(move |event| {
        event_cache.borrow_mut().update_order(&event).unwrap();
        events_handler.borrow_mut().push(event);
    }));

    let command = CancelAllOrders::new(
        TraderId::from("TRADER-001"),
        None,
        StrategyId::from("CALLER-001"),
        instrument_id,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    engine.process_cancel_all(&command, selected_account);

    let events = events.borrow();
    assert_eq!(events.len(), 1);

    let OrderEventAny::Canceled(canceled) = &events[0] else {
        panic!("Expected OrderCanceled, was {:?}", events[0]);
    };

    assert_eq!(canceled.client_order_id, selected_order.client_order_id());
    assert_eq!(canceled.strategy_id, selected_strategy);
    assert_eq!(canceled.account_id, Some(selected_account));
    let cache = cache.borrow();
    assert_eq!(
        cache
            .order(&selected_order.client_order_id())
            .unwrap()
            .status(),
        OrderStatus::Canceled
    );
    assert_eq!(
        cache
            .order(&other_order.client_order_id())
            .unwrap()
            .status(),
        OrderStatus::Submitted
    );
}

#[rstest]
fn test_process_cancel_all_excluding_leaves_excluded_orders_untouched() {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let instrument_id = instrument.id();
    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock = Rc::new(RefCell::new(VirtualClock::new()));

    let mut engine = OrderMatchingEngine::new(
        instrument,
        1,
        FillModelHandle::default(),
        FeeModelAny::MakerTaker(MakerTakerFeeModel::zero()).into(),
        BookType::L1_MBP,
        OmsType::Netting,
        AccountType::Margin,
        clock,
        Rc::clone(&cache),
        Default::default(),
    );
    let account_id = AccountId::from("ACCOUNT-001");
    let strategy_id = StrategyId::from("STRATEGY-001");
    let received = OrderTestBuilder::new(OrderType::Limit)
        .strategy_id(strategy_id)
        .instrument_id(instrument_id)
        .client_order_id(ClientOrderId::from("O-RECEIVED"))
        .side(OrderSide::Buy)
        .price(Price::from("1400.00"))
        .quantity(Quantity::from("1.000"))
        .build();
    let in_transit = OrderTestBuilder::new(OrderType::Limit)
        .strategy_id(strategy_id)
        .instrument_id(instrument_id)
        .client_order_id(ClientOrderId::from("O-IN-TRANSIT"))
        .side(OrderSide::Buy)
        .price(Price::from("1300.00"))
        .quantity(Quantity::from("1.000"))
        .build();
    {
        let mut cache = cache.borrow_mut();
        cache
            .add_order(received.clone(), None, None, false)
            .unwrap();
        cache
            .add_order(in_transit.clone(), None, None, false)
            .unwrap();
        cache
            .update_order(&TestOrderEventStubs::submitted(&received, account_id))
            .unwrap();
        cache
            .update_order(&TestOrderEventStubs::submitted(&in_transit, account_id))
            .unwrap();
    }

    let events = Rc::new(RefCell::new(Vec::new()));
    let events_handler = Rc::clone(&events);
    let event_cache = Rc::clone(&cache);
    engine.set_event_handler(Rc::new(move |event| {
        event_cache.borrow_mut().update_order(&event).unwrap();
        events_handler.borrow_mut().push(event);
    }));

    let command = CancelAllOrders::new(
        TraderId::from("TRADER-001"),
        None,
        StrategyId::from("CALLER-001"),
        instrument_id,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    engine.process_cancel_all_excluding(&command, account_id, &[in_transit.client_order_id()]);

    let events = events.borrow();
    assert_eq!(
        events.len(),
        1,
        "expected one OrderCanceled, was {events:?}"
    );

    let OrderEventAny::Canceled(canceled) = &events[0] else {
        panic!("Expected OrderCanceled, was {:?}", events[0]);
    };

    assert_eq!(canceled.client_order_id, received.client_order_id());
    assert_eq!(
        cache
            .borrow()
            .order(&in_transit.client_order_id())
            .unwrap()
            .status(),
        OrderStatus::Submitted,
        "an excluded order must be left untouched",
    );
}

#[rstest]
fn test_process_cancel_all_excluding_spares_an_excluded_contingent_order() {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let instrument_id = instrument.id();
    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock = Rc::new(RefCell::new(VirtualClock::new()));

    let mut engine = OrderMatchingEngine::new(
        instrument,
        1,
        FillModelHandle::default(),
        FeeModelAny::MakerTaker(MakerTakerFeeModel::zero()).into(),
        BookType::L1_MBP,
        OmsType::Netting,
        AccountType::Margin,
        clock,
        Rc::clone(&cache),
        Default::default(),
    );
    assert!(engine.config.support_contingent_orders);
    let account_id = AccountId::from("ACCOUNT-001");
    let strategy_id = StrategyId::from("STRATEGY-001");
    let received_id = ClientOrderId::from("O-RECEIVED");
    let in_transit_id = ClientOrderId::from("O-IN-TRANSIT");
    let received = OrderTestBuilder::new(OrderType::Limit)
        .strategy_id(strategy_id)
        .instrument_id(instrument_id)
        .client_order_id(received_id)
        .side(OrderSide::Buy)
        .price(Price::from("1400.00"))
        .quantity(Quantity::from("1.000"))
        .contingency_type(ContingencyType::Oco)
        .linked_order_ids(vec![in_transit_id])
        .build();
    let in_transit = OrderTestBuilder::new(OrderType::Limit)
        .strategy_id(strategy_id)
        .instrument_id(instrument_id)
        .client_order_id(in_transit_id)
        .side(OrderSide::Buy)
        .price(Price::from("1300.00"))
        .quantity(Quantity::from("1.000"))
        .contingency_type(ContingencyType::Oco)
        .linked_order_ids(vec![received_id])
        .build();
    {
        let mut cache = cache.borrow_mut();
        cache
            .add_order(received.clone(), None, None, false)
            .unwrap();
        cache
            .add_order(in_transit.clone(), None, None, false)
            .unwrap();
        cache
            .update_order(&TestOrderEventStubs::submitted(&received, account_id))
            .unwrap();
        cache
            .update_order(&TestOrderEventStubs::submitted(&in_transit, account_id))
            .unwrap();
    }

    let events = Rc::new(RefCell::new(Vec::new()));
    let events_handler = Rc::clone(&events);
    let event_cache = Rc::clone(&cache);
    engine.set_event_handler(Rc::new(move |event| {
        event_cache.borrow_mut().update_order(&event).unwrap();
        events_handler.borrow_mut().push(event);
    }));

    let command = CancelAllOrders::new(
        TraderId::from("TRADER-001"),
        None,
        StrategyId::from("CALLER-001"),
        instrument_id,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    engine.process_cancel_all_excluding(&command, account_id, &[in_transit_id]);

    let events = events.borrow();
    assert_eq!(
        events.len(),
        1,
        "expected one OrderCanceled, was {events:?}"
    );

    let OrderEventAny::Canceled(canceled) = &events[0] else {
        panic!("Expected OrderCanceled, was {:?}", events[0]);
    };

    assert_eq!(canceled.client_order_id, received_id);
    assert_eq!(
        cache.borrow().order(&in_transit_id).unwrap().status(),
        OrderStatus::Submitted,
        "canceling its OCO sibling must not cancel an excluded order",
    );
}

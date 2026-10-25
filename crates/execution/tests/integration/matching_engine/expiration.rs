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
#[case::futures(false, false)]
#[case::options(true, false)]
#[case::options_missing_underlying(true, true)]
fn test_expiration_settles_final_tick_fills(
    #[case] option: bool,
    #[case] missing_underlying: bool,
    #[values(false, true)] deferred: bool,
    #[values(0, 3)] initial_qty: u64,
    #[values(false, true)] use_reduce_only: bool,
    #[values(false, true)] explicit_close: bool,
    #[values(OmsType::Netting, OmsType::Hedging)] oms_type: OmsType,
) {
    let expiry = UnixNanos::from(2_000_000_000_000_000_000u64);
    let instrument = if option {
        InstrumentAny::OptionContract(option_contract("SPX", "OPRA", expiry, OptionKind::Call))
    } else {
        InstrumentAny::FuturesContract(futures_contract_es(Some(0.into()), Some(expiry)))
    };

    let account_id = AccountId::from("ACCOUNT-001");

    let (mut engine, cache, saved) = bracket_margin_engine(
        &instrument,
        account_id,
        oms_type,
        BookType::L1_MBP,
        OrderMatchingEngineConfig {
            use_reduce_only,
            defer_option_settlement: option,
            ..Default::default()
        },
    );

    let underlying = InstrumentAny::IndexInstrument(underlying_index("OPRA"));

    if option && !missing_underlying {
        cache
            .borrow_mut()
            .add_instrument(underlying.clone())
            .unwrap();
        cache
            .borrow_mut()
            .add_index_price(IndexPriceUpdate::new(
                underlying.id(),
                Price::from("160.00"),
                1.into(),
                1.into(),
            ))
            .unwrap();
    }

    engine.process_quote_tick(&QuoteTick::new(
        instrument.id(),
        Price::from("4.00"),
        Price::from("5.00"),
        Quantity::from(10),
        Quantity::from(10),
        1.into(),
        1.into(),
    ));

    if initial_qty > 0 {
        let mut opening = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("BEFORE-EXPIRY"))
            .side(OrderSide::Buy)
            .quantity(Quantity::from(initial_qty))
            .submit(true)
            .build();
        engine.process_order(&mut opening, account_id);
    }

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .client_order_id(ClientOrderId::from("FINAL-TICK"))
        .side(OrderSide::Buy)
        .price(Price::from("3.00"))
        .quantity(Quantity::from(2))
        .submit(true)
        .build();

    let position_id = cache
        .borrow()
        .positions_open(None, None, None, None, None)
        .first()
        .map(|position| position.id);
    cache
        .borrow_mut()
        .add_order(order.clone(), position_id, None, false)
        .unwrap();

    engine.process_order(&mut order, account_id);
    clear_order_event_handler_messages(&saved);

    let queued = Rc::new(RefCell::new(Vec::new()));
    if deferred {
        let events = queued.clone();
        engine.set_event_handler(Rc::new(move |event| events.borrow_mut().push(event)));
    }

    let final_time = if explicit_close {
        UnixNanos::from(expiry.as_u64() - 1)
    } else {
        expiry
    };

    let final_tick = QuoteTick::new(
        instrument.id(),
        Price::from("2.00"),
        Price::from("3.00"),
        Quantity::from(10),
        Quantity::from(10),
        final_time,
        final_time,
    );
    engine.process_quote_tick(&final_tick);
    if explicit_close {
        engine.process_instrument_close(InstrumentClose::new(
            instrument.id(),
            Price::from("7.25"),
            InstrumentCloseType::ContractExpired,
            final_time,
            final_time,
        ));
    } else {
        engine.process_instrument_expiration(expiry);
    }

    if deferred {
        assert!(!engine.is_expiration_processed());
        let pending = std::mem::take(&mut *queued.borrow_mut());
        assert_eq!(
            pending
                .iter()
                .map(OrderEventAny::event_type)
                .collect::<Vec<_>>(),
            vec![OrderEventType::Filled]
        );
        engine.process_instrument_expiration(expiry);
        assert!(queued.borrow().is_empty());

        apply_queued_events(pending);
    }

    if option && missing_underlying {
        engine.process_instrument_expiration(expiry);
        assert!(!engine.is_expiration_processed());
        assert!(queued.borrow().is_empty());
        cache
            .borrow_mut()
            .add_instrument(underlying.clone())
            .unwrap();
        cache
            .borrow_mut()
            .add_index_price(IndexPriceUpdate::new(
                underlying.id(),
                Price::from("160.00"),
                expiry,
                expiry,
            ))
            .unwrap();
    }

    engine.process_instrument_expiration(expiry);
    assert!(engine.is_expiration_processed());
    let settlement = std::mem::take(&mut *queued.borrow_mut());
    engine.process_instrument_expiration(expiry);
    engine.process_quote_tick(&final_tick);
    assert!(queued.borrow().is_empty());

    apply_queued_events(settlement);

    let events = get_order_event_handler_messages(&saved);
    assert_eq!(
        events
            .iter()
            .map(OrderEventAny::event_type)
            .collect::<Vec<_>>(),
        vec![
            OrderEventType::Filled,
            OrderEventType::Accepted,
            OrderEventType::Filled,
        ]
    );

    let [
        OrderEventAny::Filled(open),
        OrderEventAny::Accepted(accepted),
        OrderEventAny::Filled(close),
    ] = events.as_slice()
    else {
        panic!("Expected final-tick fill followed by settlement, received {events:?}");
    };

    assert_eq!(open.client_order_id, order.client_order_id());
    assert_eq!(open.order_side, OrderSide::Buy);
    assert_eq!(open.last_qty, Quantity::from(2));
    assert_eq!(open.last_px, Price::from("3.00"));
    assert_eq!(open.liquidity_side, LiquiditySide::Maker);
    assert_eq!(close.client_order_id, accepted.client_order_id);
    assert!(close.client_order_id.as_str().starts_with("EXPIRATION-"));
    assert_eq!(close.instrument_id, instrument.id());
    assert_eq!(close.account_id, account_id);
    assert_eq!(close.order_side, OrderSide::Sell);
    assert_eq!(close.last_qty, Quantity::from(initial_qty + 2));

    let close_px = match (explicit_close, option) {
        (true, _) => "7.25",
        (false, true) => "11.00",
        (false, false) => "2.00",
    };

    assert_eq!(close.last_px, Price::from(close_px));
    assert_eq!(close.liquidity_side, LiquiditySide::Taker);
    let cache = cache.borrow();
    assert_eq!(
        cache
            .positions_open(None, Some(&instrument.id()), None, None, None)
            .len(),
        0
    );
    let positions = cache.positions(None, Some(&instrument.id()), None, None, None);
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].side, PositionSide::Flat);
    assert_eq!(positions[0].quantity, Quantity::from(0));
}

#[rstest]
fn test_expiration_bar_settles_at_first_trade_tick() {
    let expiry = UnixNanos::from(2_000_000_000_000_000_000u64);
    let instrument =
        InstrumentAny::FuturesContract(futures_contract_es(Some(0.into()), Some(expiry)));
    let account_id = AccountId::from("ACCOUNT-001");
    let (mut engine, _cache, saved) = bracket_margin_engine(
        &instrument,
        account_id,
        OmsType::Netting,
        BookType::L1_MBP,
        OrderMatchingEngineConfig {
            bar_execution: true,
            ..Default::default()
        },
    );
    engine.process_quote_tick(&QuoteTick::new(
        instrument.id(),
        Price::from("4.00"),
        Price::from("5.00"),
        Quantity::from(10),
        Quantity::from(10),
        1.into(),
        1.into(),
    ));
    let mut opening = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from(1))
        .submit(true)
        .build();
    engine.process_order(&mut opening, account_id);
    clear_order_event_handler_messages(&saved);

    // No orders are working, but expiration still settles on the bar's first trade tick
    let bar_type = BarType::from(format!("{}-1-MINUTE-LAST-EXTERNAL", instrument.id()).as_str());
    for (open, high, low, close, ts) in [
        ("5.00", "5.50", "4.50", "5.25", UnixNanos::from(2)),
        ("6.00", "8.00", "5.00", "7.00", expiry),
    ] {
        engine.process_bar(&Bar::new(
            bar_type,
            Price::from(open),
            Price::from(high),
            Price::from(low),
            Price::from(close),
            Quantity::from(4),
            ts,
            ts,
        ));
    }

    let events = get_order_event_handler_messages(&saved);
    let [OrderEventAny::Accepted(_), OrderEventAny::Filled(close)] = events.as_slice() else {
        panic!("Expected expiration settlement, received {events:?}");
    };

    assert!(close.client_order_id.as_str().starts_with("EXPIRATION-"));
    assert_eq!(close.order_side, OrderSide::Sell);
    assert_eq!(close.last_qty, Quantity::from(1));
    assert_eq!(close.last_px, Price::from("6.00"));
}

#[rstest]
fn test_expiration_waits_for_position_after_order_acknowledgement(
    #[values(false, true)] option: bool,
    #[values(1, 2)] fill_qty: u64,
) {
    let expiry = UnixNanos::from(2_000_000_000_000_000_000u64);
    let instrument = if option {
        InstrumentAny::OptionContract(option_contract("SPX", "OPRA", expiry, OptionKind::Call))
    } else {
        InstrumentAny::FuturesContract(futures_contract_es(Some(0.into()), Some(expiry)))
    };

    let account_id = AccountId::from("ACCOUNT-001");
    let (mut engine, cache, saved) = bracket_margin_engine(
        &instrument,
        account_id,
        OmsType::Netting,
        BookType::L1_MBP,
        OrderMatchingEngineConfig::default(),
    );

    if option {
        let underlying = InstrumentAny::IndexInstrument(underlying_index("OPRA"));
        cache
            .borrow_mut()
            .add_instrument(underlying.clone())
            .unwrap();
        cache
            .borrow_mut()
            .add_index_price(IndexPriceUpdate::new(
                underlying.id(),
                Price::from("160.00"),
                1.into(),
                1.into(),
            ))
            .unwrap();
    }

    engine.process_quote_tick(&QuoteTick::new(
        instrument.id(),
        Price::from("4.00"),
        Price::from("5.00"),
        Quantity::from(10),
        Quantity::from(10),
        1.into(),
        1.into(),
    ));
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .client_order_id(ClientOrderId::from("FINAL-TICK"))
        .side(OrderSide::Buy)
        .price(Price::from("3.00"))
        .quantity(Quantity::from(2))
        .submit(true)
        .build();
    engine.process_order(&mut order, account_id);
    clear_order_event_handler_messages(&saved);
    let queued = Rc::new(RefCell::new(Vec::new()));
    let handler_events = queued.clone();
    engine.set_event_handler(Rc::new(move |event| {
        handler_events.borrow_mut().push(event);
    }));

    engine.process_quote_tick(&QuoteTick::new(
        instrument.id(),
        Price::from("2.00"),
        Price::from("3.00"),
        Quantity::from(10),
        Quantity::from(fill_qty),
        expiry,
        expiry,
    ));
    let events = std::mem::take(&mut *queued.borrow_mut());

    let expected_types = if fill_qty == 1 {
        vec![OrderEventType::Filled, OrderEventType::Canceled]
    } else {
        vec![OrderEventType::Filled]
    };

    assert_eq!(
        events
            .iter()
            .map(OrderEventAny::event_type)
            .collect::<Vec<_>>(),
        expected_types
    );

    for event in &events {
        cache.borrow_mut().update_order(event).unwrap();
    }

    engine.process_instrument_expiration(expiry);
    assert!(!engine.is_expiration_processed());
    assert!(queued.borrow().is_empty());
    assert_eq!(
        cache
            .borrow()
            .order(&order.client_order_id())
            .unwrap()
            .filled_qty(),
        Quantity::from(fill_qty)
    );

    let OrderEventAny::Filled(mut fill) = events[0].clone() else {
        unreachable!()
    };

    let position_id = PositionId::new(format!("{}-{}", instrument.id(), order.strategy_id()));
    fill.position_id = Some(position_id);
    let position = Position::new(&instrument, fill);
    cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();
    engine.process_instrument_expiration(expiry);
    let settlement = std::mem::take(&mut *queued.borrow_mut());

    let [OrderEventAny::Accepted(_), OrderEventAny::Filled(close)] = settlement.as_slice() else {
        panic!("Expected settlement after position acknowledgement, received {settlement:?}");
    };

    assert_eq!(close.order_side, OrderSide::Sell);
    assert_eq!(close.last_qty, Quantity::from(fill_qty));
    assert_eq!(
        close.last_px,
        Price::from(if option { "11.00" } else { "2.00" })
    );
    apply_queued_events(settlement);
    engine.process_instrument_expiration(expiry);
    assert!(engine.is_expiration_processed());
    assert!(queued.borrow().is_empty());
    assert_eq!(
        cache.borrow().position(&position_id).unwrap().side,
        PositionSide::Flat
    );
}

#[rstest]
fn test_expiration_settles_deferred_virtual_hedging_reversal() {
    let expiry = UnixNanos::from(2_000_000_000_000_000_000u64);
    let instrument =
        InstrumentAny::FuturesContract(futures_contract_es(Some(0.into()), Some(expiry)));
    let account_id = AccountId::from("ACCOUNT-001");

    let (mut engine, cache, saved) = bracket_margin_engine(
        &instrument,
        account_id,
        OmsType::Hedging,
        BookType::L2_MBP,
        OrderMatchingEngineConfig {
            use_position_ids: false,
            ..Default::default()
        },
    );

    engine
        .process_order_book_delta(&book_delta(
            &instrument,
            BookAction::Add,
            OrderSide::Sell,
            "5.00",
            "10",
            1,
        ))
        .unwrap();
    let mut opening = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument.id())
        .client_order_id(ClientOrderId::from("OPEN"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from(2))
        .submit(true)
        .build();
    engine.process_order(&mut opening, account_id);
    let source = cache.borrow().positions_open(None, None, None, None, None)[0].id;
    assert!(source.is_virtual());
    let mut reversal = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .client_order_id(ClientOrderId::from("REVERSAL"))
        .side(OrderSide::Sell)
        .price(Price::from("3.00"))
        .quantity(Quantity::from(4))
        .submit(true)
        .build();
    cache
        .borrow_mut()
        .add_order(reversal.clone(), Some(source), None, false)
        .unwrap();
    engine.process_order(&mut reversal, account_id);
    clear_order_event_handler_messages(&saved);
    let queued = Rc::new(RefCell::new(Vec::new()));
    let handler_events = queued.clone();
    engine.set_event_handler(Rc::new(move |event| {
        handler_events.borrow_mut().push(event);
    }));

    let deltas = [("4.00", "3", 2), ("3.00", "1", 3)]
        .into_iter()
        .map(|(px, qty, id)| {
            let mut delta = book_delta(&instrument, BookAction::Add, OrderSide::Buy, px, qty, id);
            delta.ts_init = expiry;
            delta
        })
        .collect();

    engine
        .process_order_book_deltas(&OrderBookDeltas::new(instrument.id(), deltas))
        .unwrap();
    let events = std::mem::take(&mut *queued.borrow_mut());
    assert_eq!(
        events
            .iter()
            .filter_map(|event| match event {
                OrderEventAny::Filled(fill) => Some(fill.last_qty),
                _ => None,
            })
            .collect::<Vec<_>>(),
        vec![Quantity::from(3), Quantity::from(1)]
    );
    assert!(!engine.is_expiration_processed());
    apply_queued_events(events);
    let destination = cache.borrow().positions_open(None, None, None, None, None)[0].id;
    assert_ne!(source, destination);
    assert_eq!(
        cache.borrow().position(&source).unwrap().side,
        PositionSide::Flat
    );
    assert_eq!(
        cache.borrow().position(&destination).unwrap().side,
        PositionSide::Short
    );
    assert_eq!(
        cache.borrow().position(&destination).unwrap().quantity,
        Quantity::from(2)
    );
    engine.process_instrument_expiration(expiry);
    assert!(engine.is_expiration_processed());
    let settlement = std::mem::take(&mut *queued.borrow_mut());

    let [OrderEventAny::Accepted(_), OrderEventAny::Filled(close)] = settlement.as_slice() else {
        panic!("Expected reversal settlement, received {settlement:?}");
    };

    assert_eq!(close.order_side, OrderSide::Buy);
    assert_eq!(close.last_qty, Quantity::from(2));
    assert_eq!(close.last_px, Price::from("5.00"));
    apply_queued_events(settlement);
    engine.process_instrument_expiration(expiry);
    assert!(queued.borrow().is_empty());
    assert_eq!(
        cache
            .borrow()
            .positions_open(None, None, None, None, None)
            .len(),
        0
    );
}

fn apply_queued_events(events: Vec<OrderEventAny>) {
    for event in events {
        msgbus::send_order_event(MessagingSwitchboard::exec_engine_process(), event);
    }
}

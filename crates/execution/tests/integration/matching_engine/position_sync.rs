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

use nautilus_model::orders::stubs::TestOrderEventStubs;

use super::*;

#[rstest]
fn test_position_fills_sync_reduce_only_orders(
    #[values(OrderSide::Buy, OrderSide::Sell)] opening_side: OrderSide,
    #[values(OmsType::Netting, OmsType::Hedging)] oms_type: OmsType,
    #[values(false, true)] deferred: bool,
    #[values(OrderType::Limit, OrderType::StopMarket, OrderType::StopLimit)]
    resting_type: OrderType,
    #[values(false, true)] support_contingent_orders: bool,
    #[values(false, true)] indexed: bool,
) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let cache = Rc::new(RefCell::new(Cache::default()));

    let mut engine = OrderMatchingEngine::new(
        instrument.clone(),
        1,
        FillModelHandle::default(),
        FeeModelAny::MakerTaker(MakerTakerFeeModel::zero()).into(),
        BookType::L2_MBP,
        oms_type,
        AccountType::Margin,
        Rc::new(RefCell::new(VirtualClock::new())),
        cache.clone(),
        OrderMatchingEngineConfig {
            support_contingent_orders,
            ..Default::default()
        },
    );

    let position_id = PositionId::from("SYNC-POSITION");

    let closing_side = if opening_side == OrderSide::Buy {
        OrderSide::Sell
    } else {
        OrderSide::Buy
    };

    let (opening, mut opening_fill) = pending_position_fill(
        &instrument,
        position_id,
        "SYNC-OPEN",
        opening_side,
        if support_contingent_orders {
            "3.000"
        } else {
            "10.000"
        },
    );

    let position_id = if oms_type == OmsType::Netting {
        PositionId::new(format!("{}-{}", instrument.id(), opening.strategy_id()))
    } else {
        position_id
    };

    opening_fill.position_id = Some(position_id);
    let mut position = Position::new(&instrument, opening_fill.clone());
    cache
        .borrow_mut()
        .add_order(opening, Some(position_id), None, false)
        .unwrap();
    cache
        .borrow_mut()
        .update_order(&OrderEventAny::Filled(opening_fill))
        .unwrap();

    for (id, qty) in [("SYNC-PARENT-A", "2.000"), ("SYNC-PARENT-B", "5.000")] {
        let (parent, mut fill) =
            pending_position_fill(&instrument, position_id, id, opening_side, qty);
        fill.venue_order_id = VenueOrderId::from(id);
        if support_contingent_orders {
            position.apply(&fill);
        }

        cache
            .borrow_mut()
            .add_order(parent, Some(position_id), None, false)
            .unwrap();

        if support_contingent_orders {
            cache
                .borrow_mut()
                .update_order(&OrderEventAny::Filled(fill))
                .unwrap();
        }
    }

    cache
        .borrow_mut()
        .add_position(&position, oms_type)
        .unwrap();
    let events = Rc::new(RefCell::new(Vec::new()));
    let events_handler = events.clone();
    let handler_cache = cache.clone();
    engine.set_event_handler(Rc::new(move |event| {
        if !deferred || matches!(event, OrderEventAny::Accepted(_)) {
            handler_cache.borrow_mut().update_order(&event).unwrap();
            if let OrderEventAny::Filled(fill) = &event {
                handler_cache
                    .borrow_mut()
                    .update_position_from_fill(position_id, fill)
                    .unwrap();
            }
        }

        events_handler.borrow_mut().push(event);
    }));

    for (id, parent, reduce_only, assigned_position) in [
        ("SYNC-A", Some("SYNC-PARENT-A"), true, position_id),
        ("SYNC-B", Some("SYNC-PARENT-B"), true, position_id),
        ("SYNC-STANDALONE", None, true, position_id),
        ("SYNC-NON-REDUCE", None, false, position_id),
        (
            "SYNC-UNRELATED",
            None,
            true,
            PositionId::from("OTHER-POSITION"),
        ),
    ] {
        let mut builder = OrderTestBuilder::new(resting_type);
        builder
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from(id))
            .side(closing_side)
            .quantity(Quantity::from("10.000"))
            .reduce_only(reduce_only)
            .submit(true);

        if resting_type != OrderType::StopMarket {
            builder.price(Price::from("2000.00"));
        }

        if resting_type != OrderType::Limit {
            builder.trigger_price(Price::from("3000.00"));
        }

        if let Some(parent) = parent {
            builder.parent_order_id(ClientOrderId::from(parent));
        }

        let mut order = builder.build();
        cache
            .borrow_mut()
            .add_order(
                order.clone(),
                if !indexed && assigned_position == position_id {
                    None
                } else {
                    Some(assigned_position)
                },
                None,
                false,
            )
            .unwrap();

        restore_order(&mut engine, &cache, &mut order, position.account_id);
    }

    let (closing, _) = pending_position_fill(
        &instrument,
        position_id,
        "SYNC-CLOSE",
        closing_side,
        "10.000",
    );
    cache
        .borrow_mut()
        .add_order(closing.clone(), Some(position_id), None, false)
        .unwrap();
    events.borrow_mut().clear();
    for (quantity, expected_updates, expected_cancels) in [
        (
            "4.000",
            vec![
                (
                    "SYNC-A",
                    if support_contingent_orders {
                        "2.000"
                    } else {
                        "6.000"
                    },
                ),
                (
                    "SYNC-B",
                    if support_contingent_orders {
                        "5.000"
                    } else {
                        "6.000"
                    },
                ),
                ("SYNC-STANDALONE", "6.000"),
            ],
            Vec::new(),
        ),
        (
            "2.000",
            if support_contingent_orders {
                vec![("SYNC-B", "4.000"), ("SYNC-STANDALONE", "4.000")]
            } else {
                vec![
                    ("SYNC-A", "4.000"),
                    ("SYNC-B", "4.000"),
                    ("SYNC-STANDALONE", "4.000"),
                ]
            },
            Vec::new(),
        ),
        (
            "4.000",
            Vec::new(),
            vec!["SYNC-A", "SYNC-B", "SYNC-STANDALONE"],
        ),
    ] {
        let start = events.borrow().len();
        set_fill_liquidity(&mut engine, closing.order_side(), quantity);
        engine.fill_market_order(closing.client_order_id());
        let events = events.borrow();
        let emitted = &events[start..];
        assert_eq!(
            emitted.len(),
            1 + expected_updates.len() + expected_cancels.len()
        );

        let OrderEventAny::Filled(fill) = &emitted[0] else {
            panic!("Expected closing fill first")
        };

        assert_eq!(fill.client_order_id, closing.client_order_id());
        assert_eq!(fill.last_qty, Quantity::from(quantity));
        assert_eq!(fill.last_px, Price::from("1000.00"));
        let mut updates = Vec::new();
        let mut cancels = Vec::new();

        for event in &emitted[1..] {
            match event {
                OrderEventAny::Updated(update) => {
                    assert_eq!(
                        update.price,
                        (resting_type != OrderType::StopMarket).then(|| Price::from("2000.00"))
                    );
                    assert_eq!(
                        update.trigger_price,
                        (resting_type != OrderType::Limit).then(|| Price::from("3000.00"))
                    );
                    updates.push((update.client_order_id.to_string(), update.quantity));
                }
                OrderEventAny::Canceled(cancel) => {
                    cancels.push(cancel.client_order_id.to_string());
                }
                other => panic!("Unexpected event {other:?}"),
            }
        }

        updates.sort_by(|a, b| a.0.cmp(&b.0));
        cancels.sort();
        assert_eq!(
            updates,
            expected_updates
                .into_iter()
                .map(|(id, qty)| (id.to_string(), Quantity::from(qty)))
                .collect::<Vec<_>>()
        );
        assert_eq!(cancels, expected_cancels);
    }

    if deferred {
        for event in events.borrow().iter() {
            cache.borrow_mut().update_order(event).unwrap();
            if let OrderEventAny::Filled(fill) = event {
                cache
                    .borrow_mut()
                    .update_position_from_fill(position_id, fill)
                    .unwrap();
            }
        }
    }

    let cache = cache.borrow();
    assert_eq!(
        cache.position(&position_id).unwrap().quantity,
        Quantity::from("0.000")
    );

    for (id, quantity) in [
        (
            "SYNC-A",
            if support_contingent_orders {
                "2.000"
            } else {
                "4.000"
            },
        ),
        ("SYNC-B", "4.000"),
        ("SYNC-STANDALONE", "4.000"),
    ] {
        let id = ClientOrderId::from(id);
        let order = cache.order(&id).unwrap();
        assert_eq!(order.status(), OrderStatus::Canceled);
        assert_eq!(order.quantity(), Quantity::from(quantity));
        assert!(!engine.order_exists(id));
    }

    for id in ["SYNC-NON-REDUCE", "SYNC-UNRELATED"] {
        let id = ClientOrderId::from(id);
        let order = cache.order(&id).unwrap();
        assert_eq!(order.status(), OrderStatus::Accepted);
        assert_eq!(order.quantity(), Quantity::from("10.000"));
        assert!(engine.order_exists(id));
    }
}

#[rstest]
#[case(None, "7.000", "11.000", OrderStatus::PartiallyFilled)]
#[case(None, "9.000", "9.000", OrderStatus::PartiallyFilled)]
#[case(None, "10.000", "10.000", OrderStatus::Canceled)]
#[case(Some("10.000"), "7.000", "10.000", OrderStatus::PartiallyFilled)]
#[case(Some("9.000"), "7.000", "9.000", OrderStatus::PartiallyFilled)]
#[case(Some("8.000"), "7.000", "8.000", OrderStatus::Canceled)]
fn test_position_sync_accounts_for_prior_fills(
    #[case] parent_filled: Option<&str>,
    #[case] closing_quantity: &str,
    #[case] expected_quantity: &str,
    #[case] expected_status: OrderStatus,
    #[values(false, true)] deferred: bool,
    #[values(false, true)] use_reduce_only: bool,
) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let position_id = PositionId::from("FLOOR-POSITION");
    let cache = Rc::new(RefCell::new(Cache::default()));

    let mut engine = OrderMatchingEngine::new(
        instrument.clone(),
        1,
        FillModelHandle::default(),
        FeeModelAny::MakerTaker(MakerTakerFeeModel::zero()).into(),
        BookType::L2_MBP,
        OmsType::Hedging,
        AccountType::Margin,
        Rc::new(RefCell::new(VirtualClock::new())),
        cache.clone(),
        OrderMatchingEngineConfig {
            use_reduce_only,
            ..Default::default()
        },
    );

    let opening_quantity = parent_filled.map_or(Quantity::from("18.000"), |quantity| {
        Quantity::from("18.000") - Quantity::from(quantity)
    });

    let (opening, opening_fill) = pending_position_fill(
        &instrument,
        position_id,
        "FLOOR-OPEN",
        OrderSide::Buy,
        &opening_quantity.to_string(),
    );
    let mut position = Position::new(&instrument, opening_fill.clone());
    cache
        .borrow_mut()
        .add_order(opening, Some(position_id), None, false)
        .unwrap();
    cache
        .borrow_mut()
        .update_order(&OrderEventAny::Filled(opening_fill))
        .unwrap();

    let parent_id = parent_filled.map(|quantity| {
        let (parent, mut fill) = pending_position_fill(
            &instrument,
            position_id,
            "FLOOR-PARENT",
            OrderSide::Buy,
            quantity,
        );
        fill.venue_order_id = VenueOrderId::from("FLOOR-PARENT");
        position.apply(&fill);
        let parent_id = parent.client_order_id();
        cache
            .borrow_mut()
            .add_order(parent, Some(position_id), None, false)
            .unwrap();
        cache
            .borrow_mut()
            .update_order(&OrderEventAny::Filled(fill))
            .unwrap();
        parent_id
    });

    let mut builder = OrderTestBuilder::new(OrderType::Limit);
    if let Some(parent_id) = parent_id {
        builder.parent_order_id(parent_id);
    }

    let mut resting = builder
        .instrument_id(instrument.id())
        .client_order_id(ClientOrderId::from("FLOOR-RESTING"))
        .side(OrderSide::Sell)
        .quantity(Quantity::from("10.000"))
        .price(Price::from("2000.00"))
        .reduce_only(true)
        .submit(true)
        .build();
    cache
        .borrow_mut()
        .add_order(resting.clone(), Some(position_id), None, false)
        .unwrap();
    let handler_cache = cache.clone();
    engine.set_event_handler(Rc::new(move |event| {
        handler_cache.borrow_mut().update_order(&event).unwrap();
    }));

    restore_order(&mut engine, &cache, &mut resting, position.account_id);
    let (_, mut prior_fill) = pending_position_fill(
        &instrument,
        position_id,
        "FLOOR-RESTING",
        OrderSide::Sell,
        "8.000",
    );
    prior_fill.venue_order_id = resting.venue_order_id().unwrap();
    prior_fill.order_type = OrderType::Limit;
    position.apply(&prior_fill);
    cache
        .borrow_mut()
        .update_order(&OrderEventAny::Filled(prior_fill))
        .unwrap();
    cache
        .borrow_mut()
        .add_position(&position, OmsType::Hedging)
        .unwrap();
    let (closing, _) = pending_position_fill(
        &instrument,
        position_id,
        "FLOOR-CLOSE",
        OrderSide::Sell,
        closing_quantity,
    );
    cache
        .borrow_mut()
        .add_order(closing.clone(), Some(position_id), None, false)
        .unwrap();
    let events = Rc::new(RefCell::new(Vec::new()));
    let events_handler = events.clone();
    let handler_cache = cache.clone();
    engine.set_event_handler(Rc::new(move |event| {
        if !deferred {
            handler_cache.borrow_mut().update_order(&event).unwrap();
            if let OrderEventAny::Filled(fill) = &event {
                handler_cache
                    .borrow_mut()
                    .update_position_from_fill(position_id, fill)
                    .unwrap();
            }
        }

        events_handler.borrow_mut().push(event);
    }));

    set_fill_liquidity(&mut engine, closing.order_side(), closing_quantity);
    engine.fill_market_order(closing.client_order_id());

    let expected_quantity = Quantity::from(if use_reduce_only {
        expected_quantity
    } else {
        "10.000"
    });

    let expected_status = if use_reduce_only {
        expected_status
    } else {
        OrderStatus::PartiallyFilled
    };

    let updated = expected_quantity != Quantity::from("10.000");
    let canceled = expected_status == OrderStatus::Canceled;
    let events = events.borrow();
    assert_eq!(
        events.len(),
        1 + usize::from(updated) + usize::from(canceled)
    );
    assert!(
        matches!(&events[0], OrderEventAny::Filled(fill) if fill.last_qty == Quantity::from(closing_quantity))
    );

    if updated {
        let OrderEventAny::Updated(update) = &events[1] else {
            panic!("Expected remaining quantity update")
        };

        assert_eq!(update.client_order_id, resting.client_order_id());
        assert_eq!(update.quantity, expected_quantity);
        assert_eq!(update.price, Some(Price::from("2000.00")));
        assert_eq!(update.trigger_price, None);
    }

    if canceled {
        let OrderEventAny::Canceled(cancel) = events.last().unwrap() else {
            panic!("Expected cancellation with no remaining capacity")
        };

        assert_eq!(cancel.client_order_id, resting.client_order_id());
    }

    if deferred {
        for event in events.iter() {
            cache.borrow_mut().update_order(event).unwrap();
            if let OrderEventAny::Filled(fill) = event {
                cache
                    .borrow_mut()
                    .update_position_from_fill(position_id, fill)
                    .unwrap();
            }
        }
    }

    let cache = cache.borrow();
    let resting = cache.order(&resting.client_order_id()).unwrap();
    assert_eq!(resting.filled_qty(), Quantity::from("8.000"));
    assert_eq!(resting.quantity(), expected_quantity);
    assert_eq!(
        resting.leaves_qty(),
        expected_quantity - Quantity::from("8.000")
    );
    assert_eq!(resting.status(), expected_status);
    assert_eq!(engine.order_exists(resting.client_order_id()), !canceled);
    assert_eq!(
        cache.position(&position_id).unwrap().quantity,
        Quantity::from("10.000") - Quantity::from(closing_quantity)
    );
}

#[rstest]
fn test_position_sync_does_not_resize_order_being_filled(#[values(false, true)] deferred: bool) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let position_id = PositionId::from("REENTRANT-POSITION");
    let cache = Rc::new(RefCell::new(Cache::default()));

    let mut engine = OrderMatchingEngine::new(
        instrument.clone(),
        1,
        FillModelHandle::default(),
        FeeModelAny::MakerTaker(MakerTakerFeeModel::zero()).into(),
        BookType::L2_MBP,
        OmsType::Hedging,
        AccountType::Margin,
        Rc::new(RefCell::new(VirtualClock::new())),
        cache.clone(),
        Default::default(),
    );
    let (opening, opening_fill) = pending_position_fill(
        &instrument,
        position_id,
        "REENTRANT-OPEN",
        OrderSide::Buy,
        "6.000",
    );
    let position = Position::new(&instrument, opening_fill.clone());
    cache
        .borrow_mut()
        .add_order(opening, Some(position_id), None, false)
        .unwrap();
    cache
        .borrow_mut()
        .update_order(&OrderEventAny::Filled(opening_fill))
        .unwrap();
    cache
        .borrow_mut()
        .add_position(&position, OmsType::Hedging)
        .unwrap();

    for (id, price, size) in [(1, "1000.00", "4.000"), (2, "999.00", "5.000")] {
        engine
            .process_order_book_delta(&OrderBookDelta::new(
                instrument.id(),
                BookAction::Add,
                BookOrder::new(OrderSide::Buy, Price::from(price), Quantity::from(size), id),
                0,
                id,
                UnixNanos::from(id),
                UnixNanos::from(id),
            ))
            .unwrap();
    }

    let events = Rc::new(RefCell::new(Vec::new()));
    let events_handler = events.clone();
    let handler_cache = cache.clone();
    engine.set_event_handler(Rc::new(move |event| {
        if !deferred || matches!(event, OrderEventAny::Accepted(_)) {
            handler_cache.borrow_mut().update_order(&event).unwrap();
            if let OrderEventAny::Filled(fill) = &event {
                handler_cache
                    .borrow_mut()
                    .update_position_from_fill(position_id, fill)
                    .unwrap();
            }
        }

        events_handler.borrow_mut().push(event);
    }));

    for (id, sibling, reduce_only) in [
        ("REENTRANT-A", "REENTRANT-B", true),
        ("REENTRANT-B", "REENTRANT-A", false),
    ] {
        let mut builder = OrderTestBuilder::new(OrderType::Limit);
        builder
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from(id))
            .side(OrderSide::Sell)
            .quantity(Quantity::from("10.000"))
            .price(Price::from(if reduce_only { "2000.00" } else { "999.00" }))
            .reduce_only(reduce_only)
            .contingency_type(ContingencyType::Ouo)
            .linked_order_ids(vec![ClientOrderId::from(sibling)])
            .submit(true);
        let mut order = builder.build();
        order.set_liquidity_side(LiquiditySide::Taker);
        cache
            .borrow_mut()
            .add_order(order.clone(), Some(position_id), None, false)
            .unwrap();
        restore_order(&mut engine, &cache, &mut order, position.account_id);
    }

    events.borrow_mut().clear();

    engine.iterate(UnixNanos::from(3), AggressorSide::NoAggressor);

    if deferred {
        for event in events.borrow().iter() {
            cache.borrow_mut().update_order(event).unwrap();
            if let OrderEventAny::Filled(fill) = event {
                cache
                    .borrow_mut()
                    .update_position_from_fill(position_id, fill)
                    .unwrap();
            }
        }
    }

    let cache = cache.borrow();
    let filled = cache.order(&ClientOrderId::from("REENTRANT-B")).unwrap();
    assert_eq!(filled.quantity(), Quantity::from("10.000"));
    assert_eq!(filled.filled_qty(), Quantity::from("9.000"));
    assert_eq!(filled.leaves_qty(), Quantity::from("1.000"));
    assert_eq!(filled.overfill_qty(), Quantity::from("0.000"));
    assert_eq!(filled.status(), OrderStatus::PartiallyFilled);
    assert_eq!(
        cache.position(&position_id).unwrap().quantity,
        Quantity::from("3.000")
    );
    assert_eq!(
        cache.position(&position_id).unwrap().side,
        PositionSide::Short
    );
    let recorded = events.borrow();

    let actual: Vec<_> = recorded
        .iter()
        .map(|event| match event {
            OrderEventAny::Filled(fill) => ("fill", fill.client_order_id.as_str(), fill.last_qty),
            OrderEventAny::Updated(update) => {
                ("update", update.client_order_id.as_str(), update.quantity)
            }
            OrderEventAny::Canceled(cancel) => {
                ("cancel", cancel.client_order_id.as_str(), Quantity::zero(3))
            }
            other => panic!("Unexpected event {other:?}"),
        })
        .collect();

    assert_eq!(
        actual,
        vec![
            ("fill", "REENTRANT-B", Quantity::from("4.000")),
            ("update", "REENTRANT-A", Quantity::from("6.000")),
            ("update", "REENTRANT-A", Quantity::from("2.000")),
            ("fill", "REENTRANT-B", Quantity::from("5.000")),
            ("update", "REENTRANT-A", Quantity::from("1.000")),
            ("cancel", "REENTRANT-A", Quantity::zero(3)),
        ]
    );
}

#[rstest]
fn test_position_sync_does_not_match_recursively(#[values(false, true)] deferred: bool) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let position_id = PositionId::from("REENTRANT-POSITION");
    let cache = Rc::new(RefCell::new(Cache::default()));

    let mut engine = OrderMatchingEngine::new(
        instrument.clone(),
        1,
        FillModelHandle::default(),
        FeeModelAny::MakerTaker(MakerTakerFeeModel::zero()).into(),
        BookType::L2_MBP,
        OmsType::Hedging,
        AccountType::Margin,
        Rc::new(RefCell::new(VirtualClock::new())),
        cache.clone(),
        Default::default(),
    );
    let (opening, opening_fill) = pending_position_fill(
        &instrument,
        position_id,
        "REENTRANT-OPEN",
        OrderSide::Buy,
        "10.000",
    );
    let position = Position::new(&instrument, opening_fill.clone());
    cache
        .borrow_mut()
        .add_order(opening, Some(position_id), None, false)
        .unwrap();
    cache
        .borrow_mut()
        .update_order(&OrderEventAny::Filled(opening_fill))
        .unwrap();
    cache
        .borrow_mut()
        .add_position(&position, OmsType::Hedging)
        .unwrap();
    let (parent, mut parent_fill) = pending_position_fill(
        &instrument,
        position_id,
        "REENTRANT-PARENT",
        OrderSide::Buy,
        "2.000",
    );
    parent_fill.venue_order_id = VenueOrderId::from("REENTRANT-PARENT");
    cache
        .borrow_mut()
        .add_order(parent, Some(position_id), None, false)
        .unwrap();
    cache
        .borrow_mut()
        .update_order(&OrderEventAny::Filled(parent_fill))
        .unwrap();

    for (id, price, size) in [(1, "1000.00", "1.000"), (2, "999.00", "9.000")] {
        engine
            .process_order_book_delta(&OrderBookDelta::new(
                instrument.id(),
                BookAction::Add,
                BookOrder::new(OrderSide::Buy, Price::from(price), Quantity::from(size), id),
                0,
                id,
                UnixNanos::from(id),
                UnixNanos::from(id),
            ))
            .unwrap();
    }

    let events = Rc::new(RefCell::new(Vec::new()));
    let events_handler = events.clone();
    let handler_cache = cache.clone();
    engine.set_event_handler(Rc::new(move |event| {
        if !deferred || matches!(event, OrderEventAny::Accepted(_)) {
            handler_cache.borrow_mut().update_order(&event).unwrap();
            if let OrderEventAny::Filled(fill) = &event {
                handler_cache
                    .borrow_mut()
                    .update_position_from_fill(position_id, fill)
                    .unwrap();
            }
        }

        events_handler.borrow_mut().push(event);
    }));

    for (id, quantity, parent) in [
        ("REENTRANT-A", "2.000", Some("REENTRANT-PARENT")),
        ("REENTRANT-B", "10.000", None),
    ] {
        let mut builder = OrderTestBuilder::new(OrderType::Limit);
        builder
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from(id))
            .side(OrderSide::Sell)
            .quantity(Quantity::from(quantity))
            .price(Price::from("999.00"))
            .reduce_only(true)
            .submit(true);

        if let Some(parent) = parent {
            builder.parent_order_id(ClientOrderId::from(parent));
        }

        let mut order = builder.build();
        order.set_liquidity_side(LiquiditySide::Taker);
        cache
            .borrow_mut()
            .add_order(order.clone(), Some(position_id), None, false)
            .unwrap();
        restore_order(&mut engine, &cache, &mut order, position.account_id);
    }

    events.borrow_mut().clear();

    assert_eq!(engine.get_open_ask_orders().len(), 2);
    assert_eq!(
        cache.borrow().position(&position_id).unwrap().quantity,
        Quantity::from("10.000")
    );
    engine.iterate(UnixNanos::from(3), AggressorSide::NoAggressor);

    let events = events.borrow();

    let fills: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => Some((
                fill.client_order_id.to_string(),
                fill.last_qty,
                fill.last_px,
            )),
            _ => None,
        })
        .collect();

    assert_eq!(
        fills,
        vec![
            (
                "REENTRANT-A".to_string(),
                Quantity::from("1.000"),
                Price::from("1000.00")
            ),
            (
                "REENTRANT-A".to_string(),
                Quantity::from("1.000"),
                Price::from("999.00")
            ),
            (
                "REENTRANT-B".to_string(),
                Quantity::from("1.000"),
                Price::from("1000.00")
            ),
            (
                "REENTRANT-B".to_string(),
                Quantity::from("7.000"),
                Price::from("999.00")
            ),
        ]
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, OrderEventAny::Canceled(_)))
    );

    if deferred {
        for event in events.iter() {
            cache.borrow_mut().update_order(event).unwrap();
            if let OrderEventAny::Filled(fill) = event {
                cache
                    .borrow_mut()
                    .update_position_from_fill(position_id, fill)
                    .unwrap();
            }
        }
    }

    let cache = cache.borrow();
    assert_eq!(
        cache.position(&position_id).unwrap().quantity,
        Quantity::from("0.000")
    );

    for (id, quantity) in [("REENTRANT-A", "2.000"), ("REENTRANT-B", "8.000")] {
        let order = cache.order(&ClientOrderId::from(id)).unwrap();
        assert_eq!(order.status(), OrderStatus::Filled);
        assert_eq!(order.quantity(), Quantity::from(quantity));
        assert_eq!(order.filled_qty(), Quantity::from(quantity));
    }
}

fn pending_position_fill(
    instrument: &InstrumentAny,
    position_id: PositionId,
    id: &str,
    side: OrderSide,
    quantity: &str,
) -> (OrderAny, OrderFilled) {
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument.id())
        .client_order_id(ClientOrderId::from(id))
        .side(side)
        .quantity(Quantity::from(quantity))
        .submit(true)
        .build();

    let OrderEventAny::Filled(fill) = TestOrderEventStubs::filled(
        &order,
        instrument,
        Some(TradeId::from(id)),
        Some(position_id),
        Some(Price::from("1000.00")),
        None,
        None,
        Some(Money::zero(instrument.quote_currency())),
        None,
        None,
    ) else {
        unreachable!()
    };

    (order, fill)
}

fn restore_order(
    engine: &mut OrderMatchingEngine,
    cache: &Rc<RefCell<Cache>>,
    order: &mut OrderAny,
    account_id: AccountId,
) {
    let venue_order_id = VenueOrderId::from(order.client_order_id().as_str());
    let accepted = TestOrderEventStubs::accepted(order, account_id, venue_order_id);
    order.apply(accepted.clone()).unwrap();
    cache.borrow_mut().update_order(&accepted).unwrap();
    engine.restore_open_order(order, account_id);
}

fn set_fill_liquidity(engine: &mut OrderMatchingEngine, side: OrderSide, size: &str) {
    let mut book = OrderBook::new(engine.instrument.id(), BookType::L2_MBP);
    book.add(
        BookOrder::new(
            side.opposite(),
            Price::from("1000.00"),
            Quantity::from(size),
            0,
        ),
        0,
        1,
        UnixNanos::from(1),
    );
    engine.set_fill_model(FillModelHandle::new(
        super::fill_models::TestBookFillModel { book },
    ));
}

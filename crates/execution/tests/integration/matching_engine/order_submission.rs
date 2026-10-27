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

use nautilus_model::events::order::spec::OrderExpiredSpec;

use super::*;

#[rstest]
#[case::long(OrderSide::Buy, ["998.00", "996.00", "1100.00", "1002.00", "990.00", "995.00"])]
#[case::short(OrderSide::Sell, ["1002.00", "1004.00", "900.00", "1010.00", "998.00", "1005.00"])]
fn test_bar_bracket_keeps_take_profit_after_stop_rejection(
    instrument_eth_usdt: InstrumentAny,
    account_id: AccountId,
    #[case] side: OrderSide,
    #[case] prices: [&str; 6],
    #[values(
        (ContingencyType::Oco, false),
        (ContingencyType::Oco, true),
        (ContingencyType::Ouo, false),
        (ContingencyType::Ouo, true)
    )]
    exits: (ContingencyType, bool),
    #[values(false, true)] deferred: bool,
    #[values(false, true)] reject_stop_orders: bool,
) {
    let [entry_price, stop_price, take_profit_price, high, low, close] = prices;
    let (contingency, take_profit_first) = exits;
    let instrument = instrument_eth_usdt;
    let cache = Rc::new(RefCell::new(Cache::default()));
    let handler = order_event_handler_with_position(cache.clone(), instrument.clone());

    let config = OrderMatchingEngineConfig {
        reject_stop_orders,
        bar_adaptive_high_low_ordering: true,
        ..Default::default()
    };

    let mut engine = get_order_matching_engine(
        instrument.clone(),
        None,
        Some(cache.clone()),
        Some(AccountType::Margin),
        Some(config),
    );
    let entry_id = ClientOrderId::from(BRACKET_ENTRY_ID);
    let stop_id = ClientOrderId::from(BRACKET_SL_ID);
    let take_profit_id = ClientOrderId::from(BRACKET_TP_ID);
    let orders = bar_bracket_orders(
        &instrument,
        side,
        [entry_price, stop_price, take_profit_price],
        contingency,
        take_profit_first,
        TimeInForce::Gtc,
    );
    submit_orders(&mut engine, account_id, orders.to_vec());
    clear_order_event_handler_messages(&handler);

    let bar = Bar {
        bar_type: BarType::from("ETHUSDT-PERP.BINANCE-15-MINUTE-LAST-EXTERNAL"),
        open: Price::from("1000.00"),
        high: Price::from(high),
        low: Price::from(low),
        close: Price::from(close),
        volume: Quantity::from("100.000"),
        ts_event: UnixNanos::from(1_000_000_000),
        ts_init: UnixNanos::from(1_000_000_000),
    };

    run_with_dispatch(&mut engine, deferred, |engine| {
        engine.process_bar(&bar);
    });

    let events = get_order_event_handler_messages(&handler);

    let rejections: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Rejected(event) => Some(event),
            _ => None,
        })
        .collect();

    {
        let cache = cache.borrow();
        let entry = cache.order(&entry_id).unwrap();
        let stop = cache.order(&stop_id).unwrap();
        let take_profit = cache.order(&take_profit_id).unwrap();
        assert_eq!(entry.status(), OrderStatus::Filled);
        assert_eq!(entry.filled_qty(), Quantity::from("1.000"));
        assert_eq!(entry.avg_px(), Some(Price::from(entry_price).as_decimal()));
        assert_eq!(take_profit.filled_qty(), Quantity::from("0.000"));

        if reject_stop_orders {
            assert_eq!(stop.status(), OrderStatus::Rejected);
            assert_eq!(stop.filled_qty(), Quantity::from("0.000"));
            assert_eq!(take_profit.status(), OrderStatus::Accepted);
            assert_eq!(rejections.len(), 1);
            assert_eq!(rejections[0].client_order_id, stop_id);

            let (bid, ask) = match side {
                OrderSide::Buy => (low, entry_price),
                OrderSide::Sell => (entry_price, high),
            };

            assert_eq!(
                rejections[0].reason,
                format!(
                    "STOP_MARKET {} order stop px of {stop_price} was in the market: bid={bid}, ask={ask}, but rejected because of configuration",
                    side.opposite(),
                ),
            );
        } else {
            assert_eq!(stop.status(), OrderStatus::Filled);
            assert_eq!(stop.filled_qty(), Quantity::from("1.000"));
            assert_eq!(stop.avg_px(), Some(Price::from(stop_price).as_decimal()));
            assert_eq!(take_profit.status(), OrderStatus::Canceled);
            assert!(rejections.is_empty());
        }
    }

    if reject_stop_orders {
        let price = Price::from(take_profit_price);

        let quote = QuoteTick::new(
            instrument.id(),
            price,
            price,
            Quantity::from("2.000"),
            Quantity::from("3.000"),
            UnixNanos::from(2_000_000_000),
            UnixNanos::from(2_000_000_000),
        );
        run_with_dispatch(&mut engine, deferred, |engine| {
            engine.process_quote_tick(&quote);
        });

        let cache = cache.borrow();
        let take_profit = cache.order(&take_profit_id).unwrap();
        assert_eq!(take_profit.status(), OrderStatus::Filled);
        assert_eq!(take_profit.filled_qty(), Quantity::from("1.000"));
        assert_eq!(take_profit.avg_px(), Some(price.as_decimal()));
        assert!(
            cache
                .positions_open(None, Some(&instrument.id()), None, None, None)
                .is_empty()
        );
        assert_eq!(
            cache.order(&stop_id).unwrap().status(),
            OrderStatus::Rejected
        );
    }
}

#[rstest]
fn test_oco_exit_does_not_cancel_rejected_sibling_during_deferred_dispatch(
    instrument_eth_usdt: InstrumentAny,
    account_id: AccountId,
    #[values(TimeInForce::Gtc, TimeInForce::Ioc, TimeInForce::Fok)] take_profit_tif: TimeInForce,
) {
    let instrument = instrument_eth_usdt;
    let cache = Rc::new(RefCell::new(Cache::default()));
    let handler = order_event_handler_with_position(cache.clone(), instrument.clone());
    let mut engine = get_order_matching_engine(
        instrument.clone(),
        None,
        Some(cache.clone()),
        Some(AccountType::Margin),
        None,
    );
    let orders = bar_bracket_orders(
        &instrument,
        OrderSide::Buy,
        ["1500.00", "100.00", "3000.00"],
        ContingencyType::Oco,
        false,
        take_profit_tif,
    );
    submit_orders(&mut engine, account_id, orders.to_vec());
    clear_order_event_handler_messages(&handler);

    let bar = Bar {
        bar_type: BarType::from("ETHUSDT-PERP.BINANCE-15-MINUTE-LAST-EXTERNAL"),
        open: Price::from("2000.00"),
        high: Price::from("3100.00"),
        low: Price::from("90.00"),
        close: Price::from("3001.00"),
        volume: Quantity::from("100.000"),
        ts_event: UnixNanos::from(1_000_000_000),
        ts_init: UnixNanos::from(1_000_000_000),
    };

    run_with_dispatch(&mut engine, true, |engine| {
        engine.process_bar(&bar);
    });

    let events = get_order_event_handler_messages(&handler);
    let stop_events: Vec<_> = events
        .iter()
        .filter(|event| event.client_order_id() == ClientOrderId::from(BRACKET_SL_ID))
        .map(OrderEventAny::event_type)
        .collect();
    let cache = cache.borrow();
    let entry = cache.order(&ClientOrderId::from(BRACKET_ENTRY_ID)).unwrap();
    let stop = cache.order(&ClientOrderId::from(BRACKET_SL_ID)).unwrap();
    let take_profit = cache.order(&ClientOrderId::from(BRACKET_TP_ID)).unwrap();
    assert_eq!(stop_events, vec![OrderEventType::Rejected]);
    assert_eq!(entry.status(), OrderStatus::Filled);
    assert_eq!(entry.filled_qty(), Quantity::from("1.000"));
    assert_eq!(entry.avg_px(), Some(dec!(1500)));
    assert_eq!(stop.status(), OrderStatus::Rejected);
    assert_eq!(stop.filled_qty(), Quantity::from("0.000"));

    let (status, filled_qty, avg_px, open_positions) = match take_profit_tif {
        TimeInForce::Gtc => (OrderStatus::Filled, "1.000", Some(dec!(3000)), 0),
        TimeInForce::Ioc | TimeInForce::Fok => (OrderStatus::Canceled, "0.000", None, 1),
        _ => unreachable!("Only GTC, IOC, and FOK are tested"),
    };

    assert_eq!(take_profit.status(), status);
    assert_eq!(take_profit.filled_qty(), Quantity::from(filled_qty));
    assert_eq!(take_profit.avg_px(), avg_px);
    assert_eq!(
        cache
            .positions_open(None, Some(&instrument.id()), None, None, None)
            .len(),
        open_positions,
    );
}

#[rstest]
fn test_oto_exit_rejection_exception_requires_rejected_same_parent_sibling(
    instrument_eth_usdt: InstrumentAny,
    account_id: AccountId,
    #[values(ContingencyType::Oco, ContingencyType::Ouo)] contingency: ContingencyType,
    #[values(None, Some(BRACKET_ENTRY_ID), Some("OTHER-ENTRY"))] sibling_parent: Option<&str>,
    #[values(
        OrderStatus::Rejected,
        OrderStatus::Canceled,
        OrderStatus::Expired,
        OrderStatus::Filled
    )]
    sibling_status: OrderStatus,
) {
    let instrument = instrument_eth_usdt;
    let cache = Rc::new(RefCell::new(Cache::default()));
    let handler = order_event_handler_with_cache(cache.clone());
    let mut engine = get_order_matching_engine(
        instrument.clone(),
        None,
        Some(cache.clone()),
        Some(AccountType::Margin),
        None,
    );
    let [parent, _, mut arriving] = bar_bracket_orders(
        &instrument,
        OrderSide::Buy,
        ["1500.00", "100.00", "3000.00"],
        contingency,
        false,
        TimeInForce::Gtc,
    );
    let parent = TestOrderStubs::make_filled_order(&parent, &instrument, LiquiditySide::Maker);

    let OrderEventAny::Filled(fill) = parent.last_event() else {
        panic!("Expected the parent fill");
    };

    let position = Position::new(&instrument, fill.clone());
    cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();
    let sibling_id = ClientOrderId::from(BRACKET_SL_ID);
    let mut sibling = OrderTestBuilder::new(OrderType::Limit);
    sibling
        .instrument_id(instrument.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from("1.000"))
        .price(Price::from("3100.00"))
        .client_order_id(sibling_id)
        .contingency_type(contingency)
        .linked_order_ids(vec![arriving.client_order_id()])
        .submit(true);

    if let Some(parent_id) = sibling_parent {
        sibling.parent_order_id(ClientOrderId::from(parent_id));
    }

    let mut sibling = sibling.build();

    match sibling_status {
        OrderStatus::Rejected => {
            let event = OrderRejectedSpec::builder()
                .client_order_id(sibling_id)
                .reason(Ustr::from("Rejected exit"))
                .build();
            sibling.apply(OrderEventAny::Rejected(event)).unwrap();
        }
        OrderStatus::Canceled => {
            sibling = TestOrderStubs::make_accepted_order(&sibling);
            let event =
                TestOrderEventStubs::canceled(&sibling, account_id, sibling.venue_order_id());
            sibling.apply(event).unwrap();
        }
        OrderStatus::Expired => {
            sibling = TestOrderStubs::make_accepted_order(&sibling);
            let event = OrderExpiredSpec::builder()
                .client_order_id(sibling_id)
                .build();
            sibling.apply(OrderEventAny::Expired(event)).unwrap();
        }
        OrderStatus::Filled => {
            sibling =
                TestOrderStubs::make_filled_order(&sibling, &instrument, LiquiditySide::Maker);
        }
        _ => unreachable!(),
    }

    cache_order(&cache, &parent);
    cache_order(&cache, &sibling);
    engine.process_order(&mut arriving, account_id);

    let expected = match (contingency, sibling_status, sibling_parent) {
        (_, OrderStatus::Rejected, Some(BRACKET_ENTRY_ID)) => OrderStatus::Accepted,
        (ContingencyType::Oco, OrderStatus::Filled, _) => OrderStatus::Canceled,
        _ => OrderStatus::Rejected,
    };

    let events = get_order_event_handler_messages(&handler);
    let event_types: Vec<_> = events.iter().map(OrderEventAny::event_type).collect();

    let event_type = match expected {
        OrderStatus::Accepted => OrderEventType::Accepted,
        OrderStatus::Canceled => OrderEventType::Canceled,
        OrderStatus::Rejected => OrderEventType::Rejected,
        _ => unreachable!(),
    };

    assert_eq!(event_types, vec![event_type]);
    assert_eq!(events[0].client_order_id(), arriving.client_order_id());
    assert_eq!(
        cache
            .borrow()
            .order(&arriving.client_order_id())
            .unwrap()
            .status(),
        expected
    );
    assert_eq!(
        cache.borrow().order(&sibling_id).unwrap().status(),
        sibling_status
    );

    if expected == OrderStatus::Rejected {
        assert_eq!(
            events[0].message(),
            Some(Ustr::from(
                format!("Contingent order {sibling_id} already closed").as_str()
            ))
        );
    }
}

fn bar_bracket_orders(
    instrument: &InstrumentAny,
    side: OrderSide,
    prices: [&str; 3],
    contingency: ContingencyType,
    take_profit_first: bool,
    take_profit_tif: TimeInForce,
) -> [OrderAny; 3] {
    let [entry_price, stop_price, take_profit_price] = prices;
    let entry_id = ClientOrderId::from(BRACKET_ENTRY_ID);
    let stop_id = ClientOrderId::from(BRACKET_SL_ID);
    let take_profit_id = ClientOrderId::from(BRACKET_TP_ID);

    let linked_order_ids = if take_profit_first {
        vec![take_profit_id, stop_id]
    } else {
        vec![stop_id, take_profit_id]
    };

    let entry = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .side(side)
        .quantity(Quantity::from("1.000"))
        .price(Price::from(entry_price))
        .client_order_id(entry_id)
        .contingency_type(ContingencyType::Oto)
        .linked_order_ids(linked_order_ids)
        .submit(true)
        .build();
    let stop = OrderTestBuilder::new(OrderType::StopMarket)
        .instrument_id(instrument.id())
        .side(side.opposite())
        .quantity(Quantity::from("1.000"))
        .trigger_price(Price::from(stop_price))
        .reduce_only(true)
        .client_order_id(stop_id)
        .parent_order_id(entry_id)
        .contingency_type(contingency)
        .linked_order_ids(vec![take_profit_id])
        .submit(true)
        .build();
    let take_profit = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .side(side.opposite())
        .quantity(Quantity::from("1.000"))
        .price(Price::from(take_profit_price))
        .time_in_force(take_profit_tif)
        .reduce_only(true)
        .client_order_id(take_profit_id)
        .parent_order_id(entry_id)
        .contingency_type(contingency)
        .linked_order_ids(vec![stop_id])
        .submit(true)
        .build();
    [entry, stop, take_profit]
}

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

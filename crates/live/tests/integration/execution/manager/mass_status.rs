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

#[tokio::test]
async fn test_reconcile_mass_status_with_empty_reports() {
    let mut ctx = TestContext::new();

    let mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert!(result.events.is_empty());
}

#[tokio::test]
async fn test_reconcile_mass_status_creates_external_order_accepted() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_id = test_client_id();

    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        None, // No client_order_id = external order
        VenueOrderId::from("V-EXT-001"),
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));

    let client_order_id = ClientOrderId::from("V-EXT-001");
    let order = ctx.get_order(&client_order_id);
    assert!(order.is_some());
    assert_eq!(
        ctx.cache.borrow().client_id(&client_order_id),
        Some(&client_id)
    );
}

#[tokio::test]
async fn test_reconcile_mass_status_materializes_restored_close_position_order() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("close-long");
    ctx.add_instrument(test_instrument());

    let report = OrderStatusReport::new(
        test_account_id(),
        instrument_id,
        Some(client_order_id),
        VenueOrderId::from("123456790"),
        OrderSide::Sell.into(),
        OrderType::StopMarket,
        TimeInForce::Gtc,
        OrderStatus::Accepted,
        Quantity::from("0.005"),
        Quantity::from("0.000"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    )
    .with_trigger_price(Price::from("2500.00"))
    .with_trigger_type(TriggerType::MarkPrice)
    .with_reduce_only(true);
    let mut mass_status = create_mass_status(vec![report], Vec::new());
    mass_status.add_position_reports(vec![PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("0.005"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3000.0)),
    )]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);
    let order = ctx.get_order(&client_order_id).unwrap();

    assert_eq!(
        result
            .events
            .iter()
            .filter(|event| matches!(
                event,
                OrderEventAny::Accepted(accepted)
                    if accepted.client_order_id == client_order_id
            ))
            .count(),
        1
    );
    assert_eq!(result.external_orders.len(), 1);
    assert_eq!(order.order_type(), OrderType::StopMarket);
    assert_eq!(order.order_side(), OrderSide::Sell);
    assert_eq!(order.quantity(), Quantity::from("0.005"));
    assert_eq!(order.trigger_price(), Some(Price::from("2500.00")));
    assert!(order.is_reduce_only());
}

#[tokio::test]
async fn test_reconcile_mass_status_rejects_external_order_with_zero_quantity() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("ordinary-zero");
    ctx.add_instrument(test_instrument());

    let report = create_order_report(
        Some(client_order_id),
        VenueOrderId::from("123456792"),
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("0.000"),
        Quantity::from("0.000"),
    );
    let mass_status = create_mass_status(vec![report], Vec::new());

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert!(result.events.is_empty());
    assert!(result.external_orders.is_empty());
    assert!(ctx.get_order(&client_order_id).is_none());
}

#[rstest]
#[case(None, true)]
#[case(None, false)]
#[case(Some("BINANCE"), true)]
#[case(Some("BINANCE"), false)]
#[case(Some("OTHER"), true)]
#[case(Some("OTHER"), false)]
#[tokio::test]
async fn test_reconcile_mass_status_warns_on_untrusted_cached_client_origin(
    #[case] cached_client_id: Option<&str>,
    #[case] report_has_client_order_id: bool,
) {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-CACHED-SOURCE");
    let venue_order_id = VenueOrderId::from("V-CACHED-SOURCE");
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .quantity(Quantity::from("1.0"))
        .price(Price::from("3000.00"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();

    ctx.add_instrument(test_instrument());
    ctx.cache
        .borrow_mut()
        .add_order(order, None, cached_client_id.map(ClientId::from), false)
        .unwrap();

    if !report_has_client_order_id {
        ctx.cache
            .borrow_mut()
            .add_venue_order_id(&client_order_id, &venue_order_id, false)
            .unwrap();
    }

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    mass_status.add_order_reports(vec![create_order_report(
        report_has_client_order_id.then_some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Canceled,
        Quantity::from("1.0"),
        Quantity::from("0"),
    )]);

    let raw_topic = MessagingSwitchboard::reconciliation_raw_order_status_report_topic();
    let raw_pattern: msgbus::MStr<msgbus::Pattern> = raw_topic.into();
    let (raw_handler, raw_saver) = get_any_saving_handler::<OrderStatusReport>(None);
    msgbus::subscribe_any(raw_pattern, raw_handler.clone(), None);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    msgbus::unsubscribe_any(raw_pattern, &raw_handler);

    assert_eq!(result.events.len(), 1);
    assert!(result.external_orders.is_empty());
    assert_eq!(
        ctx.get_order(&client_order_id).unwrap().status(),
        OrderStatus::Canceled
    );
    assert_eq!(raw_saver.get_messages().len(), 1);
    assert_eq!(
        ctx.cache.borrow().client_id(&client_order_id).copied(),
        cached_client_id.map(ClientId::from),
    );
}

#[tokio::test]
async fn test_reconcile_mass_status_warns_on_venue_only_fill_from_other_client() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-CACHED-FILL-SOURCE");
    let venue_order_id = VenueOrderId::from("V-CACHED-FILL-SOURCE");
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .quantity(Quantity::from("1.0"))
        .price(Price::from("3000.00"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();

    ctx.add_instrument(test_instrument());
    ctx.cache
        .borrow_mut()
        .add_order(order, None, Some(ClientId::from("OTHER")), false)
        .unwrap();
    ctx.cache
        .borrow_mut()
        .add_venue_order_id(&client_order_id, &venue_order_id, false)
        .unwrap();

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    mass_status.add_fill_reports(vec![FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-CACHED-FILL-SOURCE"),
        OrderSide::Buy,
        Quantity::from("1.0"),
        Price::from("3000.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Maker,
        None,
        None,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    )]);

    let raw_topic = MessagingSwitchboard::reconciliation_raw_fill_report_topic();
    let raw_pattern: msgbus::MStr<msgbus::Pattern> = raw_topic.into();
    let (raw_handler, raw_saver) = get_any_saving_handler::<FillReport>(None);
    msgbus::subscribe_any(raw_pattern, raw_handler.clone(), None);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    msgbus::unsubscribe_any(raw_pattern, &raw_handler);

    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Filled(_)));
    assert!(result.external_orders.is_empty());
    assert_eq!(raw_saver.get_messages().len(), 1);
    let cached_order = ctx.get_order(&client_order_id).unwrap();
    assert_eq!(cached_order.status(), OrderStatus::Filled);
    assert_eq!(cached_order.filled_qty(), Quantity::from("1.0"));
}

#[tokio::test]
async fn test_reconcile_mass_status_publishes_external_order_initialized() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let strategy_id = StrategyId::from("EXT-PUBLISH-MASS");

    ctx.add_instrument(test_instrument());
    ctx.manager
        .claim_external_orders(instrument_id, strategy_id)
        .unwrap();

    let topic = switchboard::get_event_order_topic(strategy_id);
    let (handler, event_messages): (_, TypedMessageSavingHandler<OrderEventAny>) =
        get_typed_message_saving_handler(None);
    msgbus::subscribe_order_events(topic.into(), handler.clone(), None);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        None,
        VenueOrderId::from("V-EXT-PUBLISH-MASS"),
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    msgbus::unsubscribe_order_events(topic.into(), &handler);

    let messages = event_messages.get_messages();
    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));
    assert_eq!(messages.len(), 2);

    match &messages[0] {
        OrderEventAny::Initialized(initialized) => {
            assert_eq!(
                initialized.client_order_id,
                ClientOrderId::from("V-EXT-PUBLISH-MASS")
            );
            assert_eq!(initialized.strategy_id, strategy_id);
            assert!(initialized.reconciliation);
        }
        event => panic!("Expected OrderInitialized event, was {event:?}"),
    }

    assert!(matches!(messages[1], OrderEventAny::Accepted(_)));
}

#[rstest]
#[case(OrderStatus::Canceled)]
#[case(OrderStatus::Rejected)]
#[case(OrderStatus::Expired)]
fn test_external_terminal_limit_without_price(#[case] status: OrderStatus) {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-IV-CLOSED");
    ctx.add_instrument(test_instrument());
    let mut report = create_order_report(
        Some(client_order_id),
        VenueOrderId::from("V-IV-CLOSED"),
        instrument_id,
        status,
        Quantity::from("0.1"),
        Quantity::from("0.0"),
    );
    report.price = None;
    let mass_status = create_mass_status(vec![report], Vec::new());

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.external_orders.len(), 1);
    let order = ctx.get_order(&client_order_id).unwrap();
    assert_eq!(order.status(), status);
    assert_eq!(order.price(), Some(Price::zero(0)));
    assert_eq!(order.quantity(), Quantity::from("0.1"));
    assert_eq!(order.filled_qty(), Quantity::from("0.0"));
    assert_eq!(order.avg_px(), None);
    assert_eq!(order.slippage(), None);
}

#[rstest]
#[case(OrderStatus::Canceled, "0.1", "0.1")]
#[case(OrderStatus::Filled, "0.3", "0.3")]
fn test_terminal_snapshot_preserves_unknown_price_after_standalone_fill(
    #[case] status: OrderStatus,
    #[case] report_quantity: &str,
    #[case] report_filled_quantity: &str,
) {
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let client_order_id = ClientOrderId::from("O-UNKNOWN-LATE-FILLED");
    let venue_order_id = VenueOrderId::from("V-UNKNOWN-LATE-FILLED");
    ctx.add_instrument(instrument.clone());
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .quantity(Quantity::from("0.1"))
        .price(Price::zero(0))
        .tags(vec![Ustr::from(RECONCILIATION_PRICE_UNKNOWN_TAG)])
        .build();

    let mut report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        if status == OrderStatus::Filled {
            OrderStatus::Canceled
        } else {
            status
        },
        Quantity::from("0.1"),
        Quantity::zero(0),
    );

    report.price = None;
    let events = generate_external_order_status_events(
        &order,
        &report,
        &test_account_id(),
        &instrument,
        UnixNanos::default(),
    );

    for event in events {
        order.apply(event).unwrap();
    }

    let fill_time = UnixNanos::from(10_000_000);
    let fill = OrderFilledTestBuilder::new(&order, &instrument)
        .last_qty(Quantity::from("0.1"))
        .last_px(Price::from("1.5"))
        .ts_event(fill_time)
        .build();
    order.apply(fill).unwrap();
    ctx.add_order(order);
    report.order_status = status;
    report.quantity = Quantity::from(report_quantity);
    report.filled_qty = Quantity::from(report_filled_quantity);
    report.price = Some(Price::from("1.3"));
    report.avg_px = Some(dec!(1.5));
    let mass_status = create_mass_status(vec![report], Vec::new());

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);
    let order = ctx.get_order(&client_order_id).unwrap();

    assert!(result.events.is_empty());
    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.ts_closed(), Some(fill_time));
    assert_eq!(order.price(), Some(Price::zero(0)));
    assert_eq!(order.quantity(), Quantity::from("0.1"));
    assert_eq!(order.filled_qty(), Quantity::from("0.1"));
    assert_eq!(order.leaves_qty(), Quantity::zero(1));
    assert_eq!(order.avg_px(), Some(dec!(1.5)));
    assert_eq!(order.slippage(), None);
    assert_eq!(
        order.tags(),
        Some([Ustr::from(RECONCILIATION_PRICE_UNKNOWN_TAG)].as_slice())
    );
}

#[tokio::test]
async fn test_reconcile_mass_status_creates_external_order_canceled() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();

    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        None,
        VenueOrderId::from("V-EXT-002"),
        instrument_id,
        OrderStatus::Canceled,
        Quantity::from("1.0"),
        Quantity::from("0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 2);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));
    assert!(matches!(result.events[1], OrderEventAny::Canceled(_)));
}

#[tokio::test]
async fn test_external_order_canceled_with_partial_fill() {
    // Test that external orders with Canceled status and partial fills
    // have both the fill and canceled events generated (matching Python behavior)
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let venue_order_id = VenueOrderId::from("V-EXT-PARTIAL");

    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Order was partially filled (0.5 of 1.0) then canceled
    let report = create_order_report(
        None,
        venue_order_id,
        instrument_id,
        OrderStatus::Canceled,
        Quantity::from("1.0"),
        Quantity::from("0.5"),
    )
    .with_avg_px(dec!(3000.00));
    mass_status.add_order_reports(vec![report]);

    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-PARTIAL-001"),
        OrderSide::Buy,
        Quantity::from("0.5"),
        Price::from("3000.00"),
        Money::from("0.25 USDT"),
        LiquiditySide::Maker,
        None, // No client_order_id for external order
        None,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );

    mass_status.add_fill_reports(vec![fill]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should have: Accepted, Filled, Canceled (in ts_event order)
    assert_eq!(result.events.len(), 3);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));
    assert!(matches!(result.events[1], OrderEventAny::Filled(_)));
    assert!(matches!(result.events[2], OrderEventAny::Canceled(_)));

    if let OrderEventAny::Filled(filled) = &result.events[1] {
        assert_eq!(filled.last_qty, Quantity::from("0.5"));
        assert_eq!(filled.trade_id, TradeId::from("T-PARTIAL-001"));
    }

    let cache = ctx.cache.borrow();
    let orders = cache.orders(None, None, None, None, None);
    assert_eq!(orders.len(), 1);
    let order = &orders[0];
    assert_eq!(order.status(), OrderStatus::Canceled);
    assert_eq!(order.filled_qty(), Quantity::from("0.5"));
}

#[rstest]
#[case(OrderStatus::Canceled)]
#[case(OrderStatus::Expired)]
#[tokio::test]
async fn test_external_terminal_order_with_incomplete_real_fills_infers_residual(
    #[case] terminal_status: OrderStatus,
) {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let venue_order_id = VenueOrderId::from("V-EXT-TERMINAL-RESIDUAL");

    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        None,
        venue_order_id,
        instrument_id,
        terminal_status,
        Quantity::from("2.0"),
        Quantity::from("1.5"),
    )
    .with_avg_px(dec!(3000.00));
    mass_status.add_order_reports(vec![report]);

    let real_trade_id = TradeId::from("T-TERMINAL-RESIDUAL-001");

    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        real_trade_id,
        OrderSide::Buy,
        Quantity::from("1.0"),
        Price::from("3000.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Maker,
        None,
        None,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );

    mass_status.add_fill_reports(vec![fill]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 4);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));
    assert!(matches!(
        (&result.events[3], terminal_status),
        (OrderEventAny::Canceled(_), OrderStatus::Canceled)
            | (OrderEventAny::Expired(_), OrderStatus::Expired),
    ));

    let OrderEventAny::Filled(real_fill) = &result.events[1] else {
        panic!("Expected real Filled event, was {:?}", result.events[1]);
    };

    let OrderEventAny::Filled(inferred_fill) = &result.events[2] else {
        panic!("Expected inferred Filled event, was {:?}", result.events[2]);
    };

    assert_eq!(real_fill.trade_id, real_trade_id);
    assert_eq!(real_fill.last_qty, Quantity::from("1.0"));
    assert_ne!(inferred_fill.trade_id, real_trade_id);
    assert_eq!(inferred_fill.last_qty, Quantity::from("0.5"));

    let cache = ctx.cache.borrow();
    let orders = cache.orders(None, None, None, None, None);
    assert_eq!(orders.len(), 1);
    assert_eq!(orders[0].status(), terminal_status);
    assert_eq!(orders[0].filled_qty(), Quantity::from("1.5"));
}

#[tokio::test]
async fn test_cached_order_canceled_with_fills() {
    // Test that a cached order transitioning to Canceled has fills applied
    // BEFORE the Canceled event (matching Python behavior)
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-CANCEL-FILL");
    let venue_order_id = VenueOrderId::from("V-CANCEL-FILL");

    ctx.add_instrument(test_instrument());

    let mut order = create_submitted_order(
        "O-CANCEL-FILL",
        instrument_id,
        OrderSide::Buy,
        "2.0",
        "3000.00",
    );
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Venue reports order was partially filled then canceled
    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Canceled,
        Quantity::from("2.0"),
        Quantity::from("1.0"),
    )
    .with_avg_px(dec!(3000.00));
    mass_status.add_order_reports(vec![report]);

    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-CACHED-001"),
        OrderSide::Buy,
        Quantity::from("1.0"),
        Price::from("3000.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Maker,
        Some(client_order_id),
        None,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );

    mass_status.add_fill_reports(vec![fill]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should have: Filled, Canceled (order already accepted)
    assert_eq!(result.events.len(), 2);
    assert!(matches!(result.events[0], OrderEventAny::Filled(_)));
    assert!(matches!(result.events[1], OrderEventAny::Canceled(_)));

    let cached_order = ctx.get_order(&client_order_id).unwrap();
    assert_eq!(cached_order.status(), OrderStatus::Canceled);
    assert_eq!(cached_order.filled_qty(), Quantity::from("1.0"));
}

#[rstest]
#[case::partial(OrderStatus::PartiallyFilled, "0.2", "0.5", dec!(3002))]
#[case::filled(OrderStatus::Filled, "1.2", "1.5", dec!(3004))]
#[tokio::test]
async fn test_mass_reconciliation_applies_replacement_alias_fills_before_inference(
    #[case] status: OrderStatus,
    #[case] child_quantity: &str,
    #[case] cumulative_quantity: &str,
    #[case] average: Decimal,
    #[values(false, true)] parent_label: bool,
    #[values(false, true)] duplicate_parent: bool,
) {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-REPLACE-MASS");
    let parent_id = VenueOrderId::from("V-REPLACE-PARENT");
    let current_id = VenueOrderId::from("V-REPLACE-CURRENT");
    let parent_trade = TradeId::from("T-REPLACE-PARENT");
    let child_trade = TradeId::from("T-REPLACE-CURRENT");
    ctx.add_instrument(test_instrument());
    let mut order = create_submitted_order(
        client_order_id.as_str(),
        instrument_id,
        OrderSide::Buy,
        "1.0",
        "3000.00",
    );
    order
        .apply(TestOrderEventStubs::accepted(
            &order,
            test_account_id(),
            parent_id,
        ))
        .unwrap();
    order
        .apply(OrderEventAny::Updated(
            OrderUpdatedSpec::builder()
                .trader_id(order.trader_id())
                .strategy_id(order.strategy_id())
                .instrument_id(instrument_id)
                .client_order_id(client_order_id)
                .account_id(test_account_id())
                .venue_order_id(current_id)
                .quantity(Quantity::from("1.5"))
                .price(Price::from("3005.00"))
                .build(),
        ))
        .unwrap();
    ctx.add_order(order);
    let report = create_order_report(
        Some(client_order_id),
        current_id,
        instrument_id,
        status,
        Quantity::from("1.5"),
        Quantity::from(cumulative_quantity),
    )
    .with_price(Price::from("3005.00"))
    .with_avg_px(average);

    let parent_fill = FillReport::new(
        test_account_id(),
        instrument_id,
        parent_id,
        parent_trade,
        OrderSide::Buy,
        Quantity::from("0.3"),
        Price::from("3000.00"),
        Money::from("0.03 USDT"),
        LiquiditySide::Maker,
        parent_label.then_some(client_order_id),
        None,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );

    let child_fill = FillReport::new(
        test_account_id(),
        instrument_id,
        current_id,
        child_trade,
        OrderSide::Buy,
        Quantity::from(child_quantity),
        Price::from("3005.00"),
        Money::from("0.05 USDT"),
        LiquiditySide::Taker,
        Some(client_order_id),
        None,
        UnixNanos::from(2_000_000),
        UnixNanos::from(2_000_000),
        None,
    );

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::from(10_000_000),
        None,
    );

    let parent_report = create_order_report(
        Some(client_order_id),
        parent_id,
        instrument_id,
        OrderStatus::Canceled,
        Quantity::from("1.0"),
        Quantity::from("0.3"),
    )
    .with_price(Price::from("3000.00"))
    .with_avg_px(dec!(3000));
    mass_status.add_order_reports(vec![parent_report, report]);
    mass_status.add_fill_reports(if duplicate_parent {
        vec![child_fill, parent_fill.clone(), parent_fill]
    } else {
        vec![child_fill, parent_fill]
    });

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);
    let repeated = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);
    let order = ctx.get_order(&client_order_id).unwrap();
    assert_eq!(order.quantity(), Quantity::from("1.5"));
    assert_eq!(order.filled_qty(), Quantity::from(cumulative_quantity));
    assert_eq!(order.avg_px(), Some(average));
    assert_eq!(order.venue_order_id(), Some(current_id));
    assert_eq!(order.status(), status);
    assert_eq!(order.trade_ids().len(), 2);
    assert!(order.trade_ids().contains(&&parent_trade));
    assert!(order.trade_ids().contains(&&child_trade));
    assert_eq!(
        order.commissions().get(&Currency::USDT()),
        Some(&Money::from("0.08 USDT"))
    );

    let fills: Vec<_> = result
        .events
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => Some(fill),
            _ => None,
        })
        .collect();

    assert_eq!(fills.len(), 2);
    assert_eq!(fills[0].venue_order_id, parent_id);
    assert_eq!(fills[0].trade_id, parent_trade);
    assert_eq!(fills[0].last_qty, Quantity::from("0.3"));
    assert_eq!(fills[0].last_px, Price::from("3000.00"));
    assert_eq!(fills[0].commission, Some(Money::from("0.03 USDT")));
    assert_eq!(fills[1].venue_order_id, current_id);
    assert_eq!(fills[1].trade_id, child_trade);
    assert_eq!(fills[1].last_qty, Quantity::from(child_quantity));
    assert_eq!(fills[1].last_px, Price::from("3005.00"));
    assert_eq!(fills[1].commission, Some(Money::from("0.05 USDT")));
    assert!(repeated.events.is_empty());
    let cache = ctx.cache.borrow();
    let positions = cache.positions_open(None, Some(&instrument_id), None, None, None);
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].quantity, Quantity::from(cumulative_quantity));
    assert_eq!(positions[0].commissions(), vec![Money::from("0.08 USDT")]);
}

#[tokio::test]
async fn test_triggered_event_generated_before_canceled() {
    // Test that Triggered event is generated before Canceled when ts_triggered is set
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-TRIG-CANCEL");
    let venue_order_id = VenueOrderId::from("V-TRIG-CANCEL");

    ctx.add_instrument(test_instrument());

    // Create and cache an accepted StopLimit order (must be triggerable)
    let mut order = OrderTestBuilder::new(OrderType::StopLimit)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.0"))
        .price(Price::from("3000.00"))
        .trigger_price(Price::from("3100.00"))
        .trigger_type(TriggerType::LastPrice)
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Venue reports order was triggered then canceled
    let mut report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Canceled,
        Quantity::from("1.0"),
        Quantity::from("0"),
    );
    report.ts_triggered = Some(UnixNanos::from(500_000));
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should have: Triggered, Canceled
    assert_eq!(result.events.len(), 2);
    assert!(matches!(result.events[0], OrderEventAny::Triggered(_)));
    assert!(matches!(result.events[1], OrderEventAny::Canceled(_)));
}

#[tokio::test]
async fn test_reconcile_mass_status_creates_external_order_filled() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();

    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        None,
        VenueOrderId::from("V-EXT-003"),
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("1.0"),
        Quantity::from("1.0"),
    )
    .with_avg_px(dec!(3000.50));
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 2);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));
    assert!(matches!(result.events[1], OrderEventAny::Filled(_)));

    if let OrderEventAny::Filled(filled) = &result.events[1] {
        assert_eq!(filled.last_qty, Quantity::from("1.0"));
        assert!(filled.reconciliation);
    }
}

#[tokio::test]
async fn test_external_order_filled_uses_real_fills() {
    // Test that external orders with Filled status use real fill reports
    // instead of inferred fills, preserving trade-level details
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let venue_order_id = VenueOrderId::from("V-EXT-FILLED");

    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Order is fully filled (2.0 of 2.0)
    let report = create_order_report(
        None,
        venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("2.0"),
        Quantity::from("2.0"),
    )
    .with_avg_px(dec!(3000.00));
    mass_status.add_order_reports(vec![report]);

    // Add two separate fill reports (multi-fill execution)
    let fill1 = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-FILL-001"),
        OrderSide::Buy,
        Quantity::from("1.0"),
        Price::from("2999.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Maker,
        None,
        None,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );

    let fill2 = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-FILL-002"),
        OrderSide::Buy,
        Quantity::from("1.0"),
        Price::from("3001.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Taker,
        None,
        None,
        UnixNanos::from(2_000_000),
        UnixNanos::from(2_000_000),
        None,
    );

    mass_status.add_fill_reports(vec![fill1, fill2]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should have: Accepted, Fill1, Fill2 (real fills, not inferred)
    assert_eq!(result.events.len(), 3);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));
    assert!(matches!(result.events[1], OrderEventAny::Filled(_)));
    assert!(matches!(result.events[2], OrderEventAny::Filled(_)));

    // Verify we got the real trade IDs, not inferred
    if let OrderEventAny::Filled(filled1) = &result.events[1] {
        assert_eq!(filled1.trade_id, TradeId::from("T-FILL-001"));
        assert_eq!(filled1.last_qty, Quantity::from("1.0"));
        assert_eq!(filled1.last_px, Price::from("2999.00"));
    }

    if let OrderEventAny::Filled(filled2) = &result.events[2] {
        assert_eq!(filled2.trade_id, TradeId::from("T-FILL-002"));
        assert_eq!(filled2.last_qty, Quantity::from("1.0"));
        assert_eq!(filled2.last_px, Price::from("3001.00"));
    }

    {
        let cache = ctx.cache.borrow();
        let orders = cache.orders(None, None, None, None, None);
        assert_eq!(orders.len(), 1);
        let order = &orders[0];
        assert_eq!(order.status(), OrderStatus::Filled);
        assert_eq!(order.filled_qty(), Quantity::from("2.0"));
    }

    ctx.manager = ExecutionManager::new(
        ctx.clock.clone(),
        ctx.cache.clone(),
        ExecutionManagerConfig::default(),
    )
    .expect("valid config");
    let replay = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);
    let cache = ctx.cache.borrow();

    assert!(replay.events.is_empty());
    assert_eq!(cache.orders(None, None, None, None, None).len(), 1);
    assert_eq!(
        cache.orders(None, None, None, None, None)[0].filled_qty(),
        Quantity::from("2.0"),
    );
}

#[tokio::test]
async fn test_external_order_filled_with_acceptance_after_fills() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-EXT-LATE-ACCEPT");
    let venue_order_id = VenueOrderId::from("V-EXT-LATE-ACCEPT");

    ctx.add_instrument(test_instrument());

    let mut report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("2.000"),
        Quantity::from("2.000"),
    )
    .with_avg_px(dec!(3000.00));

    // Venues without an acceptance time report the reconciliation time instead
    report.ts_accepted = UnixNanos::from(3_000_000);
    report.ts_last = UnixNanos::from(3_000_000);

    let fill1 = create_fill_report(
        client_order_id,
        venue_order_id,
        instrument_id,
        TradeId::from("T-LATE-001"),
        "1.000",
    );
    let mut fill2 = create_fill_report(
        client_order_id,
        venue_order_id,
        instrument_id,
        TradeId::from("T-LATE-002"),
        "1.000",
    );
    fill2.ts_event = UnixNanos::from(2_000_000);
    let mass_status = create_mass_status(vec![report], vec![fill1, fill2]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let sequence: Vec<(&str, Option<TradeId>, UnixNanos)> = result
        .events
        .iter()
        .map(|event| match event {
            OrderEventAny::Accepted(accepted) => ("accepted", None, accepted.ts_event),
            OrderEventAny::Filled(filled) => ("filled", Some(filled.trade_id), filled.ts_event),
            _ => ("other", None, event.ts_event()),
        })
        .collect();

    assert_eq!(
        sequence,
        vec![
            ("accepted", None, UnixNanos::from(3_000_000)),
            (
                "filled",
                Some(TradeId::from("T-LATE-001")),
                UnixNanos::from(1_000_000),
            ),
            (
                "filled",
                Some(TradeId::from("T-LATE-002")),
                UnixNanos::from(2_000_000),
            ),
        ]
    );

    let cache = ctx.cache.borrow();
    let orders = cache.orders(None, None, None, None, None);
    assert_eq!(orders.len(), 1);
    let order = &orders[0];
    assert_eq!(order.status(), OrderStatus::Filled);
    assert!(!order.is_open());
    assert_eq!(order.filled_qty(), Quantity::from("2.000"));
    assert_eq!(
        order.trade_ids(),
        vec![&TradeId::from("T-LATE-001"), &TradeId::from("T-LATE-002")]
    );
    assert!(cache.orders_open(None, None, None, None, None).is_empty());
    let positions = cache.positions_open(None, None, None, None, None);
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].quantity, Quantity::from("2.000"));
}

#[tokio::test]
async fn test_external_order_replaced_leg_fills_do_not_double_count() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-EXT-REPLACED");
    let old_venue_order_id = VenueOrderId::from("V-EXT-OLD");
    let new_venue_order_id = VenueOrderId::from("V-EXT-NEW");

    ctx.add_instrument(test_instrument());

    // The successor's report carries the replaced leg's filled quantity
    let mut report = create_order_report(
        Some(client_order_id),
        new_venue_order_id,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.000"),
        Quantity::from("5.000"),
    );
    report.ts_accepted = UnixNanos::from(2_000_000);
    report.ts_last = UnixNanos::from(3_000_000);

    let old_fill = create_fill_report(
        client_order_id,
        old_venue_order_id,
        instrument_id,
        TradeId::from("T-OLD"),
        "2.000",
    );
    let mut new_fill = create_fill_report(
        client_order_id,
        new_venue_order_id,
        instrument_id,
        TradeId::from("T-NEW"),
        "3.000",
    );
    new_fill.ts_event = UnixNanos::from(3_000_000);
    let mass_status = create_mass_status(vec![report], vec![old_fill, new_fill]);

    ctx.manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let cache = ctx.cache.borrow();
    let order = cache.order(&client_order_id).unwrap();
    assert_eq!(order.status(), OrderStatus::PartiallyFilled);
    assert_eq!(order.filled_qty(), Quantity::from("5.000"));
    assert_eq!(order.trade_ids().len(), 2);
    assert_eq!(order.trade_ids()[0], &TradeId::from("T-NEW"));
    assert!(!order.trade_ids().contains(&&TradeId::from("T-OLD")));
    let positions = cache.positions_open(None, None, None, None, None);
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].quantity, Quantity::from("5.000"));
}

#[tokio::test]
async fn test_external_order_filled_with_partial_fills_generates_inferred() {
    // Test that external filled orders with incomplete fill reports
    // still get an inferred fill for the remaining quantity
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let venue_order_id = VenueOrderId::from("V-EXT-PARTIAL-INFER");

    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Order is fully filled (3.0 of 3.0) according to report
    // Use precision 3 to match instrument size precision
    let report = create_order_report(
        None,
        venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("3.000"),
        Quantity::from("3.000"),
    )
    .with_avg_px(dec!(3000.00));
    mass_status.add_order_reports(vec![report]);

    // But we only have fill reports for 2.0 (missing 1.0)
    // Use precision 3 to match instrument size precision
    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-PARTIAL-001"),
        OrderSide::Buy,
        Quantity::from("2.000"),
        Price::from("2999.00"),
        Money::from("1.00 USDT"),
        LiquiditySide::Maker,
        None,
        None,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );

    mass_status.add_fill_reports(vec![fill]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should have: Accepted, RealFill (2.0), InferredFill (1.0)
    assert_eq!(result.events.len(), 3);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));
    assert!(matches!(result.events[1], OrderEventAny::Filled(_)));
    assert!(matches!(result.events[2], OrderEventAny::Filled(_)));

    // First fill is real
    if let OrderEventAny::Filled(filled1) = &result.events[1] {
        assert_eq!(filled1.trade_id, TradeId::from("T-PARTIAL-001"));
        assert_eq!(filled1.last_qty, Quantity::from("2.000"));
    }

    // Second fill is inferred (has UUID format trade_id, not the known fill report trade ID)
    if let OrderEventAny::Filled(filled2) = &result.events[2] {
        assert_ne!(
            filled2.trade_id.as_str(),
            "T-PARTIAL-001",
            "Expected inferred trade ID (UUID), was known fill report trade ID"
        );
        assert_eq!(filled2.trade_id.as_str().len(), 36);
        assert_eq!(filled2.last_qty, Quantity::from("1.000"));
    }

    let cache = ctx.cache.borrow();
    let orders = cache.orders(None, None, None, None, None);
    assert_eq!(orders.len(), 1);
    let order = &orders[0];
    assert_eq!(order.filled_qty(), Quantity::from("3.000"));
}

#[rstest]
#[case::venue_id(None)]
#[case::client_id(Some(ClientOrderId::from("O-UNCLAIMED")))]
#[tokio::test]
async fn test_reconcile_mass_status_skips_external_when_filtered(
    #[case] client_order_id: Option<ClientOrderId>,
) {
    let config = ExecutionManagerConfig {
        filter_unclaimed_external: true,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();

    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        client_order_id,
        VenueOrderId::from("V-EXT-001"),
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert!(result.events.is_empty());
    assert!(
        ctx.cache
            .borrow()
            .orders(None, None, None, None, None)
            .is_empty()
    );
}

#[tokio::test]
async fn test_synthetic_orders_bypass_filter_unclaimed_external() {
    let config = ExecutionManagerConfig {
        filter_unclaimed_external: true,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();

    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // S- prefix indicates synthetic order, should bypass filter_unclaimed_external
    let report = create_order_report(
        None,
        VenueOrderId::from("S-abc123-def456"),
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("1.0"),
        Quantity::from("1.0"),
    )
    .with_avg_px(dec!(100.0));
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert!(!result.events.is_empty());
    assert!(
        result
            .events
            .iter()
            .any(|e| matches!(e, OrderEventAny::Filled(_)))
    );

    if let OrderEventAny::Accepted(accepted) = &result.events[0] {
        let order = ctx
            .get_order(&accepted.client_order_id)
            .expect("Order should exist");
        let tags = order.tags().expect("Order should have tags");
        assert!(
            tags.contains(&ustr::Ustr::from("RECONCILIATION")),
            "Synthetic order should have RECONCILIATION tag, was {tags:?}",
        );
    } else {
        panic!("Expected Accepted event first, was {:?}", result.events[0]);
    }

    let client_order_id = result.events[0].client_order_id();
    assert_eq!(ctx.cache.borrow().client_id(&client_order_id), None);
}

#[rstest]
#[case::unfiltered_venue_id(false, None)]
#[case::filtered_venue_id(true, None)]
#[case::unfiltered_client_id(false, Some(ClientOrderId::from("O-CLAIMED")))]
#[case::filtered_client_id(true, Some(ClientOrderId::from("O-CLAIMED")))]
#[tokio::test]
async fn test_reconcile_mass_status_uses_claimed_strategy(
    #[case] filter_unclaimed_external: bool,
    #[case] client_order_id: Option<ClientOrderId>,
) {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        filter_unclaimed_external,
        ..Default::default()
    });

    let instrument_id = test_instrument_id();
    let strategy_id = StrategyId::from("MY-STRATEGY");

    ctx.add_instrument(test_instrument());
    ctx.manager
        .claim_external_orders(instrument_id, strategy_id)
        .unwrap();

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        client_order_id,
        VenueOrderId::from("V-EXT-001"),
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 1);

    let client_order_id = client_order_id.unwrap_or_else(|| ClientOrderId::from("V-EXT-001"));
    let order = ctx.get_order(&client_order_id).unwrap();
    assert_eq!(order.strategy_id(), strategy_id);
    assert_eq!(order.status(), OrderStatus::Accepted);
    assert_eq!(order.quantity(), Quantity::from("1.0"));
    assert_eq!(order.tags(), None);
}

#[rstest]
#[case::empty(false)]
#[case::partial(true)]
#[tokio::test]
async fn test_claimed_terminal_order_defers_unexplained_fills(#[case] partial: bool) {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        filter_unclaimed_external: true,
        ..Default::default()
    });

    let instrument = test_instrument();
    let strategy_id = StrategyId::from("CLAIMED-001");
    let client_order_id = ClientOrderId::from("O-CLAIMED-TERMINAL");
    let venue_order_id = VenueOrderId::from("V-CLAIMED-TERMINAL");
    ctx.add_instrument(instrument.clone());
    ctx.manager
        .claim_external_orders(instrument.id(), strategy_id)
        .unwrap();
    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument.id(),
        OrderStatus::Canceled,
        Quantity::from("10.0"),
        Quantity::from("2.0"),
    )
    .with_avg_px(dec!(3000.0));
    let first = create_fill_report(
        client_order_id,
        venue_order_id,
        instrument.id(),
        TradeId::from("T-CLAIMED-1"),
        "1.0",
    );
    let second = create_fill_report(
        client_order_id,
        venue_order_id,
        instrument.id(),
        TradeId::from("T-CLAIMED-2"),
        "1.0",
    );
    ctx.manager.reconcile_execution_mass_status(
        &create_mass_status(
            vec![report.clone()],
            if partial {
                vec![first.clone()]
            } else {
                Vec::new()
            },
        ),
        &ctx.exec_engine,
    );

    let deferred = ctx.get_order(&client_order_id);

    ctx.manager.reconcile_execution_mass_status(
        &create_mass_status(vec![report], vec![first.clone(), second.clone()]),
        &ctx.exec_engine,
    );
    ctx.exec_engine.borrow_mut().reconcile_fill_report(&first);
    ctx.exec_engine.borrow_mut().reconcile_fill_report(&second);
    let recovered = ctx.get_order(&client_order_id).unwrap();
    let cache = ctx.cache.borrow();
    let position = cache
        .position(cache.position_id(&client_order_id).unwrap())
        .unwrap();

    if partial {
        let deferred = deferred.unwrap();
        assert_eq!(deferred.status(), OrderStatus::PartiallyFilled);
        assert_eq!(deferred.filled_qty(), Quantity::from("1.0"));
        assert_eq!(deferred.strategy_id(), strategy_id);
    } else {
        assert!(deferred.is_none());
    }

    assert_eq!(recovered.status(), OrderStatus::Canceled);
    assert_eq!(recovered.filled_qty(), Quantity::from("2.0"));
    assert_eq!(recovered.strategy_id(), strategy_id);
    assert_eq!(
        recovered.trade_ids(),
        vec![&first.trade_id, &second.trade_id]
    );
    assert_eq!(
        recovered.commissions().get(&Currency::USDT()),
        Some(&Money::from("1.00 USDT"))
    );
    assert_eq!(position.quantity, Quantity::from("2.0"));
    assert_eq!(position.strategy_id, strategy_id);
}

#[tokio::test]
async fn test_claim_external_orders_duplicate_fails_without_overwriting() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let strategy_id = StrategyId::from("MY-STRATEGY");
    let duplicate_strategy_id = StrategyId::from("OTHER-STRATEGY");

    ctx.add_instrument(test_instrument());
    ctx.manager
        .claim_external_orders(instrument_id, strategy_id)
        .unwrap();

    let result = ctx
        .manager
        .claim_external_orders(instrument_id, duplicate_strategy_id);

    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("already exists for MY-STRATEGY")
    );

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        None,
        VenueOrderId::from("V-EXT-DUPLICATE"),
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 1);

    let client_order_id = ClientOrderId::from("V-EXT-DUPLICATE");
    let order = ctx.get_order(&client_order_id).unwrap();
    assert_eq!(order.strategy_id(), strategy_id);
}

#[tokio::test]
async fn test_reconcile_mass_status_processes_fills_for_cached_order() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");

    ctx.add_instrument(test_instrument());
    let order = create_limit_order("O-001", instrument_id, OrderSide::Buy, "2.0", "3000.00");
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-001"),
        OrderSide::Buy,
        Quantity::from("1.0"),
        Price::from("3000.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Maker,
        Some(client_order_id),
        None,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );

    mass_status.add_fill_reports(vec![fill]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Filled(_)));
}

#[tokio::test]
async fn test_reconcile_mass_status_deduplicates_fills() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let trade_id = TradeId::from("T-001");

    ctx.add_instrument(test_instrument());
    let order = create_limit_order("O-001", instrument_id, OrderSide::Buy, "2.0", "3000.00");
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Add same fill twice
    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        trade_id,
        OrderSide::Buy,
        Quantity::from("1.0"),
        Price::from("3000.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Maker,
        Some(client_order_id),
        None,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );

    mass_status.add_fill_reports(vec![fill.clone(), fill]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Only one fill should be processed
    assert_eq!(result.events.len(), 1);
}

#[tokio::test]
async fn test_filled_report_with_reduced_quantity_closes_partially_filled_order() {
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let client_order_id = ClientOrderId::from("O-REDUCED-FILLED");
    let venue_order_id = VenueOrderId::from("V-REDUCED-FILLED");

    ctx.add_instrument(instrument.clone());
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("10.000"))
        .price(Price::from("3000.00"))
        .build();
    apply_submitted_and_accepted(&mut order, venue_order_id);
    let fill = TestOrderEventStubs::filled(
        &order,
        &instrument,
        Some(TradeId::from("T-REDUCED-FILLED")),
        None,
        Some(Price::from("3000.00")),
        Some(Quantity::from("5.000")),
        Some(LiquiditySide::Maker),
        Some(Money::from("0.50 USDT")),
        Some(UnixNanos::from(1_000_000)),
        Some(test_account_id()),
    );
    order.apply(fill).unwrap();
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("5.000"),
        Quantity::from("5.000"),
    )
    .with_avg_px(dec!(3000.0));
    mass_status.add_order_reports(vec![report]);

    ctx.manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let order = ctx.get_order(&client_order_id).unwrap();
    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.quantity(), Quantity::from("5.000"));
    assert_eq!(order.filled_qty(), Quantity::from("5.000"));
}

#[rstest]
#[case::market(OrderType::Market, "0.00296487", "0.00296475", LiquiditySide::Taker)]
#[case::limit(
    OrderType::Limit,
    "10.00000000",
    "5.00000000",
    LiquiditySide::NoLiquiditySide
)]
#[tokio::test]
async fn test_filled_report_with_reduced_quantity_closes_accepted_order(
    #[case] order_type: OrderType,
    #[case] requested_qty: Quantity,
    #[case] filled_qty: Quantity,
    #[case] liquidity_side: LiquiditySide,
) {
    let mut ctx = TestContext::new();
    let mut pair = currency_pair_btcusdt();
    pair.size_precision = 8;
    pair.size_increment = Quantity::from("0.00000001");
    let instrument = InstrumentAny::CurrencyPair(pair);
    let instrument_id = instrument.id();
    let client_order_id = ClientOrderId::from("O-ACCEPTED-SHORT-FILL");
    let venue_order_id = VenueOrderId::from("V-ACCEPTED-SHORT-FILL");
    let commission = Money::from("0.37 USDT");

    ctx.add_instrument(instrument);
    ctx.exec_engine
        .borrow_mut()
        .deregister_client(test_client_id())
        .unwrap();
    ctx.exec_engine
        .borrow_mut()
        .register_client(Box::new(
            MockExecutionClient::new(Vec::new())
                .with_commission(commission, Rc::new(Cell::new(false))),
        ))
        .unwrap();

    let mut order = OrderTestBuilder::new(order_type)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(requested_qty)
        .price(Price::from("65000.00"))
        .build();
    apply_submitted_and_accepted(&mut order, venue_order_id);
    let mut report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        filled_qty,
        filled_qty,
    )
    .with_avg_px(dec!(65001.25));
    report.order_type = order_type;
    report.price = order.price();
    ctx.add_order(order);
    let mass_status = create_mass_status(vec![report.clone()], Vec::new());

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);
    let order = ctx.get_order(&client_order_id).unwrap();
    let replayed = OrderAny::from_events(order.events().into_iter().cloned().collect()).unwrap();
    let repeated = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);
    let mut late_fill = create_fill_report(
        client_order_id,
        venue_order_id,
        instrument_id,
        TradeId::from("T-LATE-SHORT-FILL"),
        &filled_qty.to_string(),
    );
    late_fill.last_px = Price::from("65001.25");
    late_fill.commission = commission;
    let late = ctx.manager.reconcile_execution_mass_status(
        &create_mass_status(vec![report.clone()], vec![late_fill]),
        &ctx.exec_engine,
    );
    let after_repeated = ctx.get_order(&client_order_id).unwrap();

    assert_eq!(result.events.len(), 2);

    let OrderEventAny::Updated(updated) = &result.events[0] else {
        panic!("Expected quantity update before fill");
    };

    let OrderEventAny::Filled(fill) = &result.events[1] else {
        panic!("Expected inferred fill after quantity update");
    };

    assert_eq!(updated.quantity, filled_qty);
    assert!(updated.reconciliation);
    assert_eq!(fill.client_order_id, client_order_id);
    assert_eq!(fill.venue_order_id, venue_order_id);
    assert_eq!(fill.account_id, test_account_id());
    assert_eq!(fill.instrument_id, instrument_id);
    assert_eq!(fill.order_type, order_type);
    assert_eq!(fill.order_side, OrderSide::Buy);
    assert_eq!(fill.last_qty, filled_qty);
    assert_eq!(fill.last_px, Price::from("65001.25"));
    assert_eq!(fill.currency, Currency::USDT());
    assert_eq!(fill.commission, Some(commission));
    assert_eq!(fill.liquidity_side, liquidity_side);
    assert_eq!(fill.ts_event, report.ts_last);
    assert!(fill.reconciliation);

    for reconciled in [&order, &replayed, &after_repeated] {
        assert_eq!(reconciled.status(), OrderStatus::Filled);
        assert_eq!(reconciled.quantity(), filled_qty);
        assert_eq!(reconciled.filled_qty(), filled_qty);
        assert_eq!(reconciled.leaves_qty(), Quantity::zero(8));
        assert_eq!(reconciled.avg_px(), Some(dec!(65001.25)));
        assert_eq!(reconciled.ts_closed(), Some(report.ts_last));
        assert_eq!(reconciled.trade_ids(), vec![&fill.trade_id]);
        assert_eq!(
            reconciled.commissions().get(&Currency::USDT()),
            Some(&commission)
        );
    }

    assert!(repeated.events.is_empty());
    assert!(late.events.is_empty());
    let cache = ctx.cache.borrow();
    let positions = cache.positions_open(None, Some(&instrument_id), None, None, None);
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].quantity, filled_qty);
    assert_eq!(positions[0].signed_decimal_qty(), filled_qty.as_decimal());
    assert_eq!(positions[0].commissions(), vec![commission]);
    assert_eq!(positions[0].trade_ids, AHashSet::from([fill.trade_id]));
}

#[tokio::test]
async fn test_short_fill_snapshot_retries_unavailable_commission() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-SHORT-FILL-COMMISSION");
    let venue_order_id = VenueOrderId::from("V-SHORT-FILL-COMMISSION");
    let commission = Money::from("0.29 USDT");
    let commission_failure = Rc::new(Cell::new(true));

    ctx.add_instrument(test_instrument());
    ctx.exec_engine
        .borrow_mut()
        .deregister_client(test_client_id())
        .unwrap();
    ctx.exec_engine
        .borrow_mut()
        .register_client(Box::new(
            MockExecutionClient::new(Vec::new())
                .with_commission(commission, commission_failure.clone()),
        ))
        .unwrap();
    let mut order = OrderTestBuilder::new(OrderType::Market)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("10.000"))
        .build();
    apply_submitted_and_accepted(&mut order, venue_order_id);
    ctx.add_order(order);
    let mut report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("5.000"),
        Quantity::from("5.000"),
    )
    .with_avg_px(dec!(3001.50));
    report.order_type = OrderType::Market;
    report.price = None;
    let mass_status = create_mass_status(vec![report], Vec::new());

    let failed = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);
    let unfilled = ctx.get_order(&client_order_id).unwrap();
    assert_eq!(failed.events.len(), 1);
    assert!(matches!(failed.events[0], OrderEventAny::Updated(_)));
    assert_eq!(unfilled.status(), OrderStatus::Accepted);
    assert_eq!(unfilled.quantity(), Quantity::from("5.000"));
    assert_eq!(unfilled.filled_qty(), Quantity::zero(3));
    assert_eq!(unfilled.leaves_qty(), Quantity::from("5.000"));
    assert!(unfilled.commissions().is_empty());
    assert!(
        ctx.cache
            .borrow()
            .positions_open(None, None, None, None, None)
            .is_empty()
    );

    commission_failure.set(false);
    let retried = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);
    let repeated = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);
    let order = ctx.get_order(&client_order_id).unwrap();

    assert_eq!(retried.events.len(), 1);

    let OrderEventAny::Filled(fill) = &retried.events[0] else {
        panic!("Expected inferred fill after commission becomes available");
    };

    assert_eq!(fill.last_qty, Quantity::from("5.000"));
    assert_eq!(fill.last_px, Price::from("3001.50"));
    assert_eq!(fill.commission, Some(commission));
    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.quantity(), Quantity::from("5.000"));
    assert_eq!(order.filled_qty(), Quantity::from("5.000"));
    assert_eq!(order.leaves_qty(), Quantity::zero(3));
    assert_eq!(
        order.commissions().get(&Currency::USDT()),
        Some(&commission)
    );
    assert!(repeated.events.is_empty());
    let cache = ctx.cache.borrow();
    let positions = cache.positions_open(None, Some(&instrument_id), None, None, None);
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].quantity, Quantity::from("5.000"));
    assert_eq!(positions[0].commissions(), vec![commission]);
    assert_eq!(positions[0].trade_ids, AHashSet::from([fill.trade_id]));
}

#[tokio::test]
async fn test_reconcile_mass_status_skips_order_without_instrument() {
    let mut ctx = TestContext::new();
    // Don't add instrument to cache

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        None,
        VenueOrderId::from("V-EXT-001"),
        test_instrument_id(),
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert!(result.events.is_empty());
}

#[tokio::test]
async fn test_reconcile_mass_status_sorts_events_chronologically() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");

    ctx.add_instrument(test_instrument());
    let order = create_limit_order("O-001", instrument_id, OrderSide::Buy, "2.0", "3000.00");
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Add fills in reverse chronological order
    let fill2 = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-002"),
        OrderSide::Buy,
        Quantity::from("0.5"),
        Price::from("3001.00"),
        Money::from("0.25 USDT"),
        LiquiditySide::Maker,
        Some(client_order_id),
        None,
        UnixNanos::from(2_000_000), // Later
        UnixNanos::from(2_000_000),
        None,
    );

    let fill1 = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-001"),
        OrderSide::Buy,
        Quantity::from("0.5"),
        Price::from("3000.00"),
        Money::from("0.25 USDT"),
        LiquiditySide::Maker,
        Some(client_order_id),
        None,
        UnixNanos::from(1_000_000), // Earlier
        UnixNanos::from(1_000_000),
        None,
    );

    mass_status.add_fill_reports(vec![fill2, fill1]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 2);

    // Verify chronological ordering
    assert!(result.events[0].ts_event() < result.events[1].ts_event());
}

#[tokio::test]
async fn test_reconcile_mass_status_accepted_order_canceled_at_venue() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");

    ctx.add_instrument(test_instrument());

    // Create and accept order locally
    let mut order =
        create_submitted_order("O-001", instrument_id, OrderSide::Buy, "1.0", "3000.00");

    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order);

    // Venue reports order was canceled
    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Canceled,
        Quantity::from("1.0"),
        Quantity::from("0"),
    )
    .with_cancel_reason("USER_REQUEST".to_string());
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Canceled(_)));

    if let OrderEventAny::Canceled(canceled) = &result.events[0] {
        assert_eq!(canceled.client_order_id, client_order_id);
        assert!(canceled.reconciliation); // Verify reconciliation flag is set
    }
}

#[tokio::test]
async fn test_reconcile_mass_status_accepted_order_expired_at_venue() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-002");
    let venue_order_id = VenueOrderId::from("V-002");

    ctx.add_instrument(test_instrument());

    // Create and accept order locally
    let mut order =
        create_submitted_order("O-002", instrument_id, OrderSide::Sell, "2.0", "3100.00");

    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order);

    // Venue reports order expired
    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Expired,
        Quantity::from("2.0"),
        Quantity::from("0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Expired(_)));
}

#[tokio::test]
async fn test_reconcile_mass_status_external_order_partially_filled() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();

    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        None, // External order
        VenueOrderId::from("V-EXT-PARTIAL"),
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("3.0"),
    )
    .with_avg_px(dec!(3000.50));
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // External orders get: Accepted + Filled (for the partial fill)
    assert_eq!(result.events.len(), 2);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));
    assert!(matches!(result.events[1], OrderEventAny::Filled(_)));

    if let OrderEventAny::Filled(filled) = &result.events[1] {
        assert_eq!(filled.last_qty, Quantity::from("3.0"));
        assert!(filled.reconciliation);
    }

    // Verify order was created in cache (status is Initialized since events haven't been applied)
    let client_order_id = ClientOrderId::from("V-EXT-PARTIAL");
    let order = ctx.get_order(&client_order_id);
    assert!(order.is_some());
}

#[tokio::test]
async fn test_reconcile_mass_status_order_already_in_sync() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-SYNC");
    let venue_order_id = VenueOrderId::from("V-SYNC");

    ctx.add_instrument(test_instrument());

    // Create accepted order locally
    let mut order =
        create_submitted_order("O-SYNC", instrument_id, OrderSide::Buy, "5.0", "3000.00");
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order);

    // Venue reports exact same state
    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("5.0"),
        Quantity::from("0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // No events needed - already in sync
    assert!(result.events.is_empty());
}

#[tokio::test]
async fn test_inferred_fill_generated_when_venue_reports_filled() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-FILL-001");
    let venue_order_id = VenueOrderId::from("V-FILL-001");

    ctx.add_instrument(test_instrument());

    // Create accepted order with no fills yet
    let order = create_accepted_order(
        "O-FILL-001",
        instrument_id,
        OrderSide::Buy,
        "10.0",
        "3000.00",
        venue_order_id,
    );
    ctx.add_order(order);

    // Venue reports order as partially filled (no FillReport, just status)
    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("5.0"), // 5 filled
    )
    .with_avg_px(dec!(3001.50));
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should generate an inferred fill
    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Filled(_)));

    if let OrderEventAny::Filled(filled) = &result.events[0] {
        assert_eq!(filled.client_order_id, client_order_id);
        assert_eq!(filled.last_qty, Quantity::from("5.0"));
        assert!(filled.reconciliation);
        assert_eq!(filled.trade_id.as_str().len(), 36);
    }
}

#[rstest]
#[case::filled(OrderStatus::Filled, "10.0", 1)]
#[case::canceled(OrderStatus::Canceled, "4.5", 2)]
#[case::expired(OrderStatus::Expired, "4.5", 2)]
#[tokio::test]
async fn test_mass_status_retries_missing_fill_data(
    #[case] status: OrderStatus,
    #[case] filled_qty: Quantity,
    #[case] event_count: usize,
) {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-COMMISSION-MASS-001");
    let venue_order_id = VenueOrderId::from("V-COMMISSION-MASS-001");
    let commission = Money::from("1.25 USDT");
    let commission_failure = Rc::new(Cell::new(true));

    ctx.add_instrument(test_instrument());
    let order = create_accepted_order(
        "O-COMMISSION-MASS-001",
        instrument_id,
        OrderSide::Buy,
        "10.0",
        "3000.00",
        venue_order_id,
    );
    ctx.add_order_with_client_id(order, test_client_id());
    ctx.exec_engine
        .borrow_mut()
        .deregister_client(test_client_id())
        .expect("generic test execution client deregisters");
    ctx.exec_engine
        .borrow_mut()
        .register_client(Box::new(
            MockExecutionClient::new(Vec::new())
                .with_commission(commission, commission_failure.clone()),
        ))
        .expect("test execution client registers");

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        status,
        Quantity::from("10.0"),
        filled_qty,
    )
    .with_avg_px(dec!(3001.50));
    let failed_mass_status = create_mass_status(vec![report.clone()], Vec::new());

    let failed = ctx
        .manager
        .reconcile_execution_mass_status(&failed_mass_status, &ctx.exec_engine);

    assert!(failed.events.is_empty());
    let order = ctx
        .get_order(&client_order_id)
        .expect("cached order remains available for retry");
    assert_eq!(order.status(), OrderStatus::Accepted);
    assert_eq!(order.filled_qty(), Quantity::from("0.0"));
    assert_eq!(order.commissions().get(&Currency::USDT()), None);

    commission_failure.set(false);
    let trade_id = TradeId::from("T-TERMINAL-MASS-001");

    let fills = if status == OrderStatus::Filled {
        Vec::new()
    } else {
        let mut fill = create_fill_report(
            client_order_id,
            venue_order_id,
            instrument_id,
            trade_id,
            "4.5",
        );
        fill.last_px = Price::from("3001.50");
        fill.commission = commission;
        vec![fill]
    };

    let retry_mass_status = create_mass_status(vec![report.clone()], fills);
    let retry = ctx
        .manager
        .reconcile_execution_mass_status(&retry_mass_status, &ctx.exec_engine);

    assert_eq!(retry.events.len(), event_count);

    let OrderEventAny::Filled(fill) = &retry.events[0] else {
        panic!("expected fill on valid retry");
    };

    assert_eq!(fill.client_order_id, client_order_id);
    assert_eq!(fill.last_qty, filled_qty);
    assert_eq!(fill.last_px, Price::from("3001.50"));
    assert_eq!(fill.commission, Some(commission));
    assert!(fill.reconciliation);

    if status != OrderStatus::Filled {
        assert_eq!(fill.trade_id, trade_id);
    }

    let order = ctx
        .get_order(&client_order_id)
        .expect("retry updates the cached order");
    assert_eq!(order.status(), status);
    assert_eq!(order.filled_qty(), filled_qty);
    assert_eq!(
        order.commissions().get(&Currency::USDT()),
        Some(&commission)
    );

    let repeated = ctx.manager.reconcile_execution_mass_status(
        &create_mass_status(vec![report], Vec::new()),
        &ctx.exec_engine,
    );

    assert!(repeated.events.is_empty());
}

#[tokio::test]
async fn test_inferred_fill_uses_avg_px_for_first_fill() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-AVG-001");
    let venue_order_id = VenueOrderId::from("V-AVG-001");

    ctx.add_instrument(test_instrument());

    let order = create_accepted_order(
        "O-AVG-001",
        instrument_id,
        OrderSide::Buy,
        "10.0",
        "3000.00",
        venue_order_id,
    );
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("3.0"),
    )
    .with_avg_px(dec!(2999.75));
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 1);

    if let OrderEventAny::Filled(filled) = &result.events[0] {
        // First fill should use avg_px directly
        assert_eq!(filled.last_px.as_f64(), 2999.75);
    }
}

#[tokio::test]
async fn test_no_inferred_fill_when_already_in_sync() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-SYNC-001");
    let venue_order_id = VenueOrderId::from("V-SYNC-001");

    ctx.add_instrument(test_instrument());

    // Create an order that is already partially filled
    let mut order = create_accepted_order(
        "O-SYNC-001",
        instrument_id,
        OrderSide::Buy,
        "10.0",
        "3000.00",
        venue_order_id,
    );

    // Apply a fill to the order
    let fill = TestOrderEventStubs::filled(
        &order,
        &test_instrument(),
        None,                        // trade_id
        None,                        // position_id
        None,                        // last_px
        Some(Quantity::from("5.0")), // last_qty
        None,                        // liquidity_side
        None,                        // commission
        None,                        // ts_filled_ns
        None,                        // account_id
    );
    order.apply(fill).unwrap();
    ctx.add_order(order);

    // Venue reports same fill state
    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("5.0"), // Same as local
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // No events needed - already in sync
    assert!(result.events.is_empty());
}

#[rstest]
#[case(false)]
#[case(true)]
#[tokio::test]
async fn test_fill_qty_mismatch_venue_less_generates_fill_void(#[case] echo_cached_fill: bool) {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-MISMATCH");
    let venue_order_id = VenueOrderId::from("V-MISMATCH");
    let trade_id = TradeId::from("T-MISMATCH");

    ctx.add_instrument(test_instrument());

    // Create an order that is already partially filled with 5
    let mut order = create_accepted_order(
        "O-MISMATCH",
        instrument_id,
        OrderSide::Buy,
        "10.0",
        "3000.00",
        venue_order_id,
    );
    let fill = OrderFilledTestBuilder::new(&order, &test_instrument())
        .trade_id(trade_id)
        .account_id(test_account_id())
        .last_qty(Quantity::from("5.0"))
        .without_position_id()
        .build();
    order.apply(fill).unwrap();
    ctx.add_order(order);

    // Venue reports less filled than Nautilus has applied.
    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("3.0"), // Less than our 5
    );
    mass_status.add_order_reports(vec![report]);
    if echo_cached_fill {
        mass_status.add_fill_reports(vec![create_fill_report(
            client_order_id,
            venue_order_id,
            instrument_id,
            trade_id,
            "5.0",
        )]);
    }

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let order = ctx
        .cache
        .borrow()
        .order_owned(&client_order_id)
        .expect("order cached");

    let expected_events = if echo_cached_fill {
        // The echoed fill report prices the trade at 3000.00 while the cached fill recorded
        // 1.0, so reconciliation first restates the trade's price, then voids the decrease
        let OrderEventAny::Filled(restatement) = &result.events[0] else {
            panic!("expected price restatement fill event");
        };

        assert_eq!(restatement.trade_id, trade_id);
        assert_eq!(restatement.last_qty, Quantity::from("5.0"));
        assert_eq!(restatement.last_px, Price::from("3000.00"));
        2
    } else {
        1
    };

    assert_eq!(result.events.len(), expected_events);

    let OrderEventAny::FillVoided(voided) = &result.events[expected_events - 1] else {
        panic!("expected OrderFillVoided event");
    };

    assert_eq!(voided.client_order_id, client_order_id);
    assert_eq!(voided.trade_id, trade_id);
    assert_eq!(voided.voided_qty, Quantity::from("2.0"));
    assert!(voided.is_reopened);
    assert_eq!(order.status(), OrderStatus::PartiallyFilled);
    assert_eq!(order.filled_qty(), Quantity::from("3.0"));
    assert_eq!(order.voided_qty(), Quantity::from("2.0"));
    assert_eq!(
        order.avg_px(),
        if echo_cached_fill {
            Some(dec!(3000.00))
        } else {
            Some(dec!(1.0))
        }
    );
}

#[tokio::test]
async fn test_market_order_inferred_fill_is_taker() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-MKT-001");
    let venue_order_id = VenueOrderId::from("V-MKT-001");

    ctx.add_instrument(test_instrument());

    // Create a market order (submitted and accepted)
    let mut order = OrderTestBuilder::new(OrderType::Market)
        .client_order_id(ClientOrderId::from("O-MKT-001"))
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("10.0"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = OrderStatusReport::new(
        test_account_id(),
        instrument_id,
        Some(client_order_id),
        venue_order_id,
        OrderSide::Buy.into(),
        OrderType::Market,
        TimeInForce::Ioc,
        OrderStatus::Filled,
        Quantity::from("10.0"),
        Quantity::from("10.0"),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        Some(UUID4::new()),
    )
    .with_avg_px(dec!(3005.00));
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 1);

    if let OrderEventAny::Filled(filled) = &result.events[0] {
        assert_eq!(filled.liquidity_side, LiquiditySide::Taker);
    }
}

#[tokio::test]
async fn test_pending_cancel_status_no_event() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-PEND-001");
    let venue_order_id = VenueOrderId::from("V-PEND-001");

    ctx.add_instrument(test_instrument());

    let order = create_accepted_order(
        "O-PEND-001",
        instrument_id,
        OrderSide::Buy,
        "10.0",
        "3000.00",
        venue_order_id,
    );
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PendingCancel,
        Quantity::from("10.0"),
        Quantity::from("0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Pending states don't generate events
    assert!(result.events.is_empty());
}

#[tokio::test]
async fn test_incremental_fill_calculates_weighted_price() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-INCR-001");
    let venue_order_id = VenueOrderId::from("V-INCR-001");

    ctx.add_instrument(test_instrument());

    // Create an order that already has 5 filled at 3000.00
    let mut order = create_accepted_order(
        "O-INCR-001",
        instrument_id,
        OrderSide::Buy,
        "10.0",
        "3000.00",
        venue_order_id,
    );
    let fill = TestOrderEventStubs::filled(
        &order,
        &test_instrument(),
        None,                         // trade_id
        None,                         // position_id
        Some(Price::from("3000.00")), // last_px
        Some(Quantity::from("5.0")),  // last_qty
        None,                         // liquidity_side
        None,                         // commission
        None,                         // ts_filled_ns
        None,                         // account_id
    );
    order.apply(fill).unwrap();
    ctx.add_order(order);

    // Venue reports 8 filled total at avg_px 3002.50
    // Original: 5 @ 3000.00 = 15000
    // New avg: 8 @ 3002.50 = 24020
    // Incremental: 3 @ (24020 - 15000) / 3 = 3006.67
    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("8.0"),
    )
    .with_avg_px(dec!(3002.50));
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 1);

    if let OrderEventAny::Filled(filled) = &result.events[0] {
        assert_eq!(filled.last_qty, Quantity::from("3.0"));
        // (8 * 3002.50 - 5 * 3000.00) / 3 ≈ 3006.67
        let expected_px = (8.0 * 3002.50 - 5.0 * 3000.00) / 3.0;
        assert!((filled.last_px.as_f64() - expected_px).abs() < 0.01);
    }
}

#[rstest]
#[tokio::test]
async fn test_mass_status_skips_exact_duplicate_orders() {
    let mut ctx = TestContext::new();
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let instrument_id = test_instrument_id();

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .quantity(Quantity::from("1.0"))
        .price(Price::from("100.0"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0.0"),
    )
    .with_price(Price::from("100.0"));
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert!(result.events.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_mass_status_deduplicates_within_batch() {
    let mut ctx = TestContext::new();
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let instrument_id = test_instrument_id();

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .quantity(Quantity::from("1.0"))
        .price(Price::from("100.0"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report1 = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0.0"),
    );
    let report2 = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0.0"),
    );
    mass_status.add_order_reports(vec![report1, report2]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));
}

#[rstest]
#[tokio::test]
async fn test_mass_status_reconciles_when_status_differs() {
    let mut ctx = TestContext::new();
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let instrument_id = test_instrument_id();

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .quantity(Quantity::from("1.0"))
        .price(Price::from("100.0"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Canceled,
        Quantity::from("1.0"),
        Quantity::from("0.0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Canceled(_)));
}

#[rstest]
#[tokio::test]
async fn test_mass_status_reconciles_when_filled_qty_differs() {
    let mut ctx = TestContext::new();
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let instrument_id = test_instrument_id();

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .quantity(Quantity::from("10.0"))
        .price(Price::from("100.0"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("5.0"),
    )
    .with_avg_px(dec!(100.0));
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 1);

    if let OrderEventAny::Filled(filled) = &result.events[0] {
        assert_eq!(filled.last_qty, Quantity::from("5.0"));
    } else {
        panic!("Expected OrderFilled event");
    }
}

#[rstest]
#[tokio::test]
async fn test_mass_status_matches_order_by_venue_order_id() {
    let mut ctx = TestContext::new();
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let instrument_id = test_instrument_id();

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .quantity(Quantity::from("1.0"))
        .price(Price::from("100.0"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order);

    ctx.cache
        .borrow_mut()
        .add_venue_order_id(&client_order_id, &venue_order_id, false)
        .unwrap();

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        None,
        venue_order_id,
        instrument_id,
        OrderStatus::Canceled,
        Quantity::from("1.0"),
        Quantity::from("0.0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Canceled(_)));

    if let OrderEventAny::Canceled(canceled) = &result.events[0] {
        assert_eq!(canceled.client_order_id, client_order_id);
    }
}

#[rstest]
#[tokio::test]
async fn test_mass_status_matches_order_by_venue_order_id_with_mismatched_client_id() {
    let mut ctx = TestContext::new();
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let instrument_id = test_instrument_id();

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .quantity(Quantity::from("1.0"))
        .price(Price::from("100.0"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order);

    ctx.cache
        .borrow_mut()
        .add_venue_order_id(&client_order_id, &venue_order_id, false)
        .unwrap();

    // Report has wrong client_order_id but correct venue_order_id
    let wrong_client_order_id = ClientOrderId::from("O-WRONG");

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        Some(wrong_client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Canceled,
        Quantity::from("1.0"),
        Quantity::from("0.0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Canceled(_)));

    if let OrderEventAny::Canceled(canceled) = &result.events[0] {
        assert_eq!(canceled.client_order_id, client_order_id);
    }
}

#[tokio::test]
async fn test_reconcile_mass_status_indexes_venue_order_id_for_accepted_orders() {
    // Test that venue_order_id is properly indexed during reconciliation for orders
    // that are already in ACCEPTED state and don't generate new OrderAccepted events.
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let instrument = test_instrument();
    ctx.add_instrument(instrument);

    let client_order_id = ClientOrderId::from("O-TEST");
    let venue_order_id = VenueOrderId::from("V-123");

    let mut order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.0"))
        .client_order_id(client_order_id)
        .build();

    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();

    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order.clone());

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0.0"),
    );

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        Venue::from("SIM"),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    mass_status.add_order_reports(vec![report]);

    let _events = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(
        ctx.cache.borrow().client_order_id(&venue_order_id),
        Some(&client_order_id),
        "venue_order_id should be indexed after reconciliation"
    );
}

#[tokio::test]
async fn test_reconcile_mass_status_indexes_venue_order_id_for_external_orders() {
    // Test that venue_order_id is properly indexed for external orders discovered
    // during reconciliation.
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let instrument = test_instrument();
    ctx.add_instrument(instrument);

    let venue_order_id = VenueOrderId::from("V-EXT-001");

    let report = create_order_report(
        None,
        venue_order_id,
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0.0"),
    );

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        Venue::from("SIM"),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert!(
        !result.events.is_empty(),
        "Should generate events for external order"
    );

    let cache_borrow = ctx.cache.borrow();
    let indexed_client_id = cache_borrow.client_order_id(&venue_order_id);
    assert!(
        indexed_client_id.is_some(),
        "venue_order_id should be indexed for external order"
    );
}

#[tokio::test]
async fn test_reconcile_mass_status_indexes_venue_order_id_for_filled_orders() {
    // Test that venue_order_id is properly indexed for orders that are already
    // FILLED and don't generate new OrderAccepted events.
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let instrument = test_instrument();
    ctx.add_instrument(instrument.clone());

    let client_order_id = ClientOrderId::from("O-FILLED");
    let venue_order_id = VenueOrderId::from("V-456");

    // Create order and process to FILLED state
    let mut order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.0"))
        .client_order_id(client_order_id)
        .build();

    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();

    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();

    let filled = TestOrderEventStubs::filled(
        &order,
        &instrument,
        Some(TradeId::from("T-1")),
        None,                          // position_id
        Some(Price::from("1.0")),      // last_px
        Some(Quantity::from("1.0")),   // last_qty
        Some(LiquiditySide::Taker),    // liquidity_side
        Some(Money::from("0.01 USD")), // commission
        None,                          // ts_filled_ns
        Some(test_account_id()),
    );
    order.apply(filled).unwrap();
    ctx.add_order(order.clone());

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("1.0"),
        Quantity::from("1.0"),
    );

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        Venue::from("SIM"),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    mass_status.add_order_reports(vec![report]);

    let _events = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let cache_borrow = ctx.cache.borrow();
    assert_eq!(
        cache_borrow.client_order_id(&venue_order_id),
        Some(&client_order_id),
        "venue_order_id should be indexed for filled order"
    );
}

#[tokio::test]
async fn test_reconcile_mass_status_skips_orders_without_loaded_instruments() {
    // Test that reconciliation properly skips orders for instruments that aren't loaded,
    // and these skipped orders don't cause validation warnings.
    let mut ctx = TestContext::new();
    let loaded_instrument_id = test_instrument_id();
    let loaded_instrument = test_instrument();
    ctx.add_instrument(loaded_instrument);

    let unloaded_instrument_id = InstrumentId::from("BTCUSDT.SIM");

    let loaded_venue_order_id = VenueOrderId::from("V-LOADED");
    let unloaded_venue_order_id = VenueOrderId::from("V-UNLOADED");

    let loaded_report = create_order_report(
        Some(ClientOrderId::from("O-LOADED")),
        loaded_venue_order_id,
        loaded_instrument_id,
        OrderStatus::Filled,
        Quantity::from("1.0"),
        Quantity::from("1.0"),
    );

    let unloaded_report = create_order_report(
        Some(ClientOrderId::from("O-UNLOADED")),
        unloaded_venue_order_id,
        unloaded_instrument_id,
        OrderStatus::Filled,
        Quantity::from("1.0"),
        Quantity::from("1.0"),
    );

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        Venue::from("SIM"),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    mass_status.add_order_reports(vec![loaded_report, unloaded_report]);

    let _events = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let cache_borrow = ctx.cache.borrow();
    let loaded_client_id = cache_borrow.client_order_id(&loaded_venue_order_id);
    assert!(
        loaded_client_id.is_some(),
        "Loaded instrument order should be indexed"
    );

    let unloaded_client_id = cache_borrow.client_order_id(&unloaded_venue_order_id);
    assert!(
        unloaded_client_id.is_none(),
        "Unloaded instrument order should not be indexed (skipped during reconciliation)"
    );
}

#[rstest]
#[case::netting(false)]
#[case::hedging(true)]
#[tokio::test]
async fn test_missing_orders_disabled_skips_synthetic_fill_recovery(#[case] hedging: bool) {
    let config = ExecutionManagerConfig {
        generate_missing_orders: false,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let venue_position_id = hedging.then(|| PositionId::from("ETHUSDT-PERP.BINANCE-LONG"));
    mass_status.add_fill_reports(vec![FillReport::new(
        test_account_id(),
        instrument_id,
        VenueOrderId::from("V-PARTIAL-DISABLED"),
        TradeId::from("T-PARTIAL-DISABLED"),
        OrderSide::Buy,
        Quantity::from("2.000"),
        Price::from("3100.00"),
        Money::from("1.00 USDT"),
        LiquiditySide::Taker,
        None,
        venue_position_id,
        UnixNanos::from(1_000_001),
        UnixNanos::from(1_000_001),
        None,
    )]);
    mass_status.add_position_reports(vec![PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.000"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        venue_position_id,
        Some(dec!(3000.00)),
    )]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let cache = ctx.cache.borrow();

    assert!(result.events.is_empty());
    assert!(result.external_orders.is_empty());
    assert_eq!(cache.orders_total_count(None, None, None, None, None), 0);
    assert!(cache.positions(None, None, None, None, None).is_empty());
}

#[tokio::test]
async fn test_external_order_has_venue_tag() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let venue_order_id = VenueOrderId::from("V-EXT-001");

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // External order with no client_order_id
    let report = create_order_report(
        None,
        venue_order_id,
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0.0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert!(!result.events.is_empty());

    // Get the order created and verify it has the VENUE tag
    let client_order_id = ClientOrderId::from("V-EXT-001");
    let order = ctx.get_order(&client_order_id).expect("Order should exist");
    let tags = order.tags().expect("Order should have tags");
    assert!(
        tags.contains(&ustr::Ustr::from("VENUE")),
        "External order should have VENUE tag"
    );
}

#[rstest]
#[case::accepted_filled(
    OrderStatus::Accepted,
    OrderType::Market,
    "0.0",
    "10.0",
    OrderStatus::Filled
)]
#[case::accepted_partial(
    OrderStatus::Accepted,
    OrderType::Market,
    "0.0",
    "6.0",
    OrderStatus::PartiallyFilled
)]
#[case::accepted_priced_residual(
    OrderStatus::Accepted,
    OrderType::Limit,
    "6.0",
    "4.0",
    OrderStatus::PartiallyFilled
)]
#[case::triggered_filled(
    OrderStatus::Triggered,
    OrderType::StopMarket,
    "0.0",
    "10.0",
    OrderStatus::Filled
)]
#[case::triggered_partial(
    OrderStatus::Triggered,
    OrderType::StopMarket,
    "0.0",
    "6.0",
    OrderStatus::PartiallyFilled
)]
#[case::triggered_priced_residual(
    OrderStatus::Triggered,
    OrderType::StopLimit,
    "6.0",
    "4.0",
    OrderStatus::PartiallyFilled
)]
#[case::filled(
    OrderStatus::Filled,
    OrderType::Market,
    "10.0",
    "10.0",
    OrderStatus::Filled
)]
#[tokio::test]
async fn test_external_order_with_fills_but_no_avg_px_applies_real_fills_only(
    #[case] order_status: OrderStatus,
    #[case] order_type: OrderType,
    #[case] reported_filled_qty: &str,
    #[case] fill_qty: &str,
    #[case] expected_status: OrderStatus,
) {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let venue_order_id = VenueOrderId::from("V-FILLS-001");

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let mut report = OrderStatusReport::new(
        test_account_id(),
        instrument_id,
        None, // External order
        venue_order_id,
        OrderSide::Buy.into(),
        order_type,
        TimeInForce::Gtc,
        order_status,
        Quantity::from("10.0"),
        Quantity::from(reported_filled_qty),
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
        None,
    );

    if matches!(order_type, OrderType::Limit | OrderType::StopLimit) {
        report = report.with_price(Price::from("3000.00"));
    }

    if matches!(order_type, OrderType::StopMarket | OrderType::StopLimit) {
        report = report
            .with_trigger_price(Price::from("2900.00"))
            .with_trigger_type(TriggerType::MarkPrice);
    }

    // Real fill report with actual price
    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-REAL-001"),
        OrderSide::Buy,
        Quantity::from(fill_qty),
        Price::from("3000.00"),
        Money::from("1.00 USDT"),
        LiquiditySide::Taker,
        None,
        None,
        UnixNanos::from(1_000_001),
        UnixNanos::from(1_000_001),
        None,
    );

    mass_status.add_order_reports(vec![report]);
    mass_status.add_fill_reports(vec![fill]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should have events including Accepted and the real fill
    let accepted_count = result
        .events
        .iter()
        .filter(|e| matches!(e, OrderEventAny::Accepted(_)))
        .count();
    assert_eq!(accepted_count, 1, "Should have exactly 1 Accepted event");

    let fill_events: Vec<_> = result
        .events
        .iter()
        .filter_map(|e| {
            if let OrderEventAny::Filled(f) = e {
                Some(f)
            } else {
                None
            }
        })
        .collect();

    assert_eq!(
        fill_events.len(),
        1,
        "Should have exactly 1 fill (the real one)"
    );
    assert_eq!(
        fill_events[0].trade_id,
        TradeId::from("T-REAL-001"),
        "Fill should be the real fill, not an inferred one"
    );
    assert_eq!(result.events.len(), 2);
    assert_eq!(fill_events[0].last_qty, Quantity::from(fill_qty));
    assert_eq!(fill_events[0].last_px, Price::from("3000.00"));
    assert_eq!(fill_events[0].commission, Some(Money::from("1.00 USDT")));
    assert_eq!(fill_events[0].liquidity_side, LiquiditySide::Taker);
    assert_eq!(fill_events[0].ts_event, UnixNanos::from(1_000_001));
    assert!(fill_events[0].reconciliation);

    let client_order_id = ClientOrderId::from("V-FILLS-001");
    let order = ctx.get_order(&client_order_id).expect("Order should exist");
    assert_eq!(order.status(), expected_status);
    assert_eq!(order.quantity(), Quantity::from("10.0"));
    assert_eq!(order.filled_qty(), Quantity::from(fill_qty));
}

#[tokio::test]
async fn test_closed_reconciliation_orders_skipped_on_restart() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-RECON-001");
    let venue_order_id = VenueOrderId::from("V-RECON-001");

    // Create a closed reconciliation order from a previous session
    let mut order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("5.0"))
        .client_order_id(client_order_id)
        .tags(vec![ustr::Ustr::from("RECONCILIATION")])
        .build();

    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    let filled = TestOrderEventStubs::filled(
        &order,
        &test_instrument(),
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

    assert!(order.is_closed());
    ctx.add_order(order);

    // Simulate restart with a mass status that contains this order
    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("5.0"),
        Quantity::from("5.0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should skip the closed reconciliation order - no new events generated
    assert!(
        result.events.is_empty(),
        "Should skip closed RECONCILIATION order, but got {} events",
        result.events.len()
    );
}

#[tokio::test]
async fn test_expired_order_applies_fills_before_terminal_event() {
    // Expired orders should apply fills before the expired event (same as canceled)
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let instrument = test_instrument();
    ctx.add_instrument(instrument);

    let client_order_id = ClientOrderId::from("O-EXPIRE-TEST");
    let venue_order_id = VenueOrderId::from("V-EXPIRE-001");

    // Create and submit an order
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("10.0"))
        .price(Price::from("100.0"))
        .client_order_id(client_order_id)
        .build();

    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order.clone());
    ctx.cache
        .borrow_mut()
        .add_venue_order_id(&client_order_id, &venue_order_id, false)
        .unwrap();

    // Report shows order EXPIRED with partial fills
    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Expired,
        Quantity::from("10.0"),
        Quantity::from("3.0"), // Partially filled
    );
    mass_status.add_order_reports(vec![report]);

    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-001"),
        OrderSide::Buy,
        Quantity::from("3.0"),
        Price::from("100.0"),
        Money::from("0.10 USDT"),
        LiquiditySide::Maker,
        Some(client_order_id),
        None,
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
        None,
    );

    mass_status.add_fill_reports(vec![fill]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should have Fill event BEFORE Expired event
    let fill_count = result
        .events
        .iter()
        .filter(|e| matches!(e, OrderEventAny::Filled(_)))
        .count();
    let expired_count = result
        .events
        .iter()
        .filter(|e| matches!(e, OrderEventAny::Expired(_)))
        .count();

    assert_eq!(fill_count, 1, "Should have 1 fill event");
    assert_eq!(expired_count, 1, "Should have 1 expired event");

    // Verify fill comes before expired in the event list
    let fill_idx = result
        .events
        .iter()
        .position(|e| matches!(e, OrderEventAny::Filled(_)))
        .unwrap();
    let expired_idx = result
        .events
        .iter()
        .position(|e| matches!(e, OrderEventAny::Expired(_)))
        .unwrap();

    assert!(
        fill_idx < expired_idx,
        "Fill event should come before Expired event"
    );
}

#[tokio::test]
async fn test_mass_status_preserves_symbol_scoped_trade_ids() {
    let mut ctx = TestContext::new();
    let instrument1 = test_instrument();
    let instrument2 = InstrumentAny::CurrencyPair(currency_pair_btcusdt());
    let instrument_id1 = instrument1.id();
    let instrument_id2 = instrument2.id();
    let client_order_id1 = ClientOrderId::from("O-SCOPED-ETH");
    let client_order_id2 = ClientOrderId::from("O-SCOPED-BTC");
    let venue_order_id1 = VenueOrderId::from("V-SCOPED-ETH");
    let venue_order_id2 = VenueOrderId::from("V-SCOPED-BTC");

    ctx.add_instrument(instrument1);
    ctx.add_instrument(instrument2);
    ctx.add_order(create_accepted_order(
        client_order_id1.as_str(),
        instrument_id1,
        OrderSide::Buy,
        "1.000",
        "3000.00",
        venue_order_id1,
    ));
    ctx.add_order(create_accepted_order(
        client_order_id2.as_str(),
        instrument_id2,
        OrderSide::Buy,
        "0.100000",
        "50000.00",
        venue_order_id2,
    ));

    let shared_trade_id = TradeId::from("12345678");

    let fill1 = FillReport::new(
        test_account_id(),
        instrument_id1,
        venue_order_id1,
        shared_trade_id,
        OrderSide::Buy,
        Quantity::from("1.000"),
        Price::from("3000.00"),
        Money::from("0.10 USDT"),
        LiquiditySide::Taker,
        Some(client_order_id1),
        None,
        UnixNanos::from(1_000_001),
        UnixNanos::from(1_000_001),
        None,
    );

    let fill2 = FillReport::new(
        test_account_id(),
        instrument_id2,
        venue_order_id2,
        shared_trade_id,
        OrderSide::Buy,
        Quantity::from("0.100000"),
        Price::from("50000.00"),
        Money::from("0.10 USDT"),
        LiquiditySide::Taker,
        Some(client_order_id2),
        None,
        UnixNanos::from(1_000_002),
        UnixNanos::from(1_000_002),
        None,
    );

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    mass_status.add_fill_reports(vec![fill1, fill2]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let fill_instruments: HashSet<_> = result
        .events
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => Some(fill.instrument_id),
            _ => None,
        })
        .collect();

    assert_eq!(
        fill_instruments,
        HashSet::from([instrument_id1, instrument_id2])
    );
}

#[tokio::test]
async fn test_duplicate_order_reports_reconciles_last_report() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let venue_order_id = VenueOrderId::from("V-DUP-001");
    let client_order_id = ClientOrderId::from("O-DUP-001");

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .quantity(Quantity::from("10.0"))
        .price(Price::from("3000.00"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order);
    ctx.cache
        .borrow_mut()
        .add_venue_order_id(&client_order_id, &venue_order_id, false)
        .unwrap();

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report_partial = OrderStatusReport::new(
        test_account_id(),
        instrument_id,
        Some(client_order_id),
        venue_order_id,
        OrderSide::Buy.into(),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("5.0"),
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
        None,
    );

    let report_filled = OrderStatusReport::new(
        test_account_id(),
        instrument_id,
        Some(client_order_id),
        venue_order_id,
        OrderSide::Buy.into(),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Filled,
        Quantity::from("10.0"),
        Quantity::from("10.0"),
        UnixNanos::from(2_000),
        UnixNanos::from(2_000),
        UnixNanos::from(2_000),
        None,
    )
    .with_avg_px(dec!(3000.0));

    mass_status.add_order_reports(vec![report_partial, report_filled]);

    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-001"),
        OrderSide::Buy,
        Quantity::from("10.0"),
        Price::from("3000.00"),
        Money::from("1.00 USDT"),
        LiquiditySide::Taker,
        None,
        None,
        UnixNanos::from(2_000),
        UnixNanos::from(2_000),
        None,
    );

    mass_status.add_fill_reports(vec![fill]);

    let _result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let order = ctx.get_order(&client_order_id).expect("Order should exist");
    assert_eq!(
        order.status(),
        OrderStatus::Filled,
        "Order should match the last report (Filled)"
    );
}

#[tokio::test]
async fn test_reconciliation_order_skipped_on_restart() {
    // Closed reconciliation orders (with RECONCILIATION tag) should be skipped on restart
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let venue_order_id = VenueOrderId::from("S-RECON-001");
    let client_order_id = ClientOrderId::from("S-RECON-001");

    let mut order = OrderTestBuilder::new(OrderType::Market)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .quantity(Quantity::from("5.0"))
        .tags(vec![ustr::Ustr::from("RECONCILIATION")])
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    let filled = TestOrderEventStubs::filled(
        &order,
        &test_instrument(),
        None,
        None,
        Some(Price::from("3000.00")),
        None,
        None,
        None,
        None,
        None,
    );
    order.apply(filled).unwrap();
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Report the same order again (simulating restart)
    let report = OrderStatusReport::new(
        test_account_id(),
        instrument_id,
        Some(client_order_id),
        venue_order_id,
        OrderSide::Buy.into(),
        OrderType::Market,
        TimeInForce::Gtc,
        OrderStatus::Filled,
        Quantity::from("5.0"),
        Quantity::from("5.0"),
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
        None,
    )
    .with_avg_px(dec!(3000.0));
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert!(
        result.events.is_empty(),
        "Closed reconciliation order should be skipped on restart"
    );
}

#[tokio::test]
async fn test_partially_filled_order_has_fills_applied() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let venue_order_id = VenueOrderId::from("V-PARTIAL-001");
    let client_order_id = ClientOrderId::from("O-PARTIAL-001");

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .quantity(Quantity::from("10.0"))
        .price(Price::from("3000.00"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    ctx.add_order(order);
    ctx.cache
        .borrow_mut()
        .add_venue_order_id(&client_order_id, &venue_order_id, false)
        .unwrap();

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("5.0"),
    )
    .with_avg_px(dec!(3000.0));

    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-001"),
        OrderSide::Buy,
        Quantity::from("5.0"),
        Price::from("3000.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Maker,
        None,
        None,
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
        None,
    );

    mass_status.add_order_reports(vec![report]);
    mass_status.add_fill_reports(vec![fill]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let has_fills = result
        .events
        .iter()
        .any(|e| matches!(e, OrderEventAny::Filled(_)));
    assert!(has_fills, "Should generate fill events");

    let order = ctx.get_order(&client_order_id).expect("Order should exist");
    assert!(
        order.filled_qty() >= Quantity::from("5.0"),
        "Order should have at least 5.0 filled"
    );
}

#[rstest]
#[case(OrderStatus::PartiallyFilled, "7.0", true)]
#[case(OrderStatus::Filled, "10.0", true)]
#[case(OrderStatus::PartiallyFilled, "7.0", false)]
#[case(OrderStatus::Filled, "10.0", false)]
#[tokio::test]
async fn test_cached_fill_echo_preserves_mass_status_projection(
    #[case] status: OrderStatus,
    #[case] filled_qty: &str,
    #[case] has_new_fill: bool,
) {
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let client_order_id = ClientOrderId::from("O-CACHED-ECHO");
    let venue_order_id = VenueOrderId::from("V-CACHED-ECHO");
    let cached_trade_id = TradeId::from("T-CACHED-ECHO");
    let new_trade_id = TradeId::from("T-NEW-ECHO");
    ctx.add_instrument(instrument.clone());
    let mut order = create_accepted_order(
        client_order_id.as_str(),
        instrument_id,
        OrderSide::Buy,
        "10.0",
        "3000.00",
        venue_order_id,
    );
    let cached_fill = TestOrderEventStubs::filled(
        &order,
        &instrument,
        Some(cached_trade_id),
        None,
        Some(Price::from("3000.00")),
        Some(Quantity::from("3.0")),
        Some(LiquiditySide::Maker),
        None,
        None,
        Some(test_account_id()),
    );
    order.apply(cached_fill).unwrap();
    ctx.add_order(order);

    let filled_qty = Quantity::from(filled_qty);
    let remaining_qty = filled_qty - Quantity::from("3.0");
    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        status,
        Quantity::from("10.0"),
        filled_qty,
    )
    .with_avg_px(dec!(3000));
    let mut fills = vec![create_fill_report(
        client_order_id,
        venue_order_id,
        instrument_id,
        cached_trade_id,
        "3.0",
    )];

    if has_new_fill {
        let mut fill = create_fill_report(
            client_order_id,
            venue_order_id,
            instrument_id,
            new_trade_id,
            &remaining_qty.to_string(),
        );
        fill.ts_event = UnixNanos::from(2_000_000);
        fills.push(fill);
    }

    let result = ctx.manager.reconcile_execution_mass_status(
        &create_mass_status(vec![report], fills),
        &ctx.exec_engine,
    );

    let [OrderEventAny::Filled(fill)] = result.events.as_slice() else {
        panic!(
            "Expected one incremental fill, received {:?}",
            result.events
        );
    };

    assert_eq!(fill.client_order_id, client_order_id);
    assert_eq!(fill.venue_order_id, venue_order_id);
    assert_eq!(fill.account_id, test_account_id());
    assert_eq!(fill.instrument_id, instrument_id);
    assert_eq!(fill.last_qty, remaining_qty);
    assert_eq!(fill.last_px, Price::from("3000.00"));

    if has_new_fill {
        assert_eq!(fill.trade_id, new_trade_id);
    } else {
        assert_ne!(fill.trade_id, cached_trade_id);
    }

    let order = ctx.get_order(&client_order_id).unwrap();
    assert_eq!(order.status(), status);
    assert_eq!(order.filled_qty(), filled_qty);
    assert_eq!(order.trade_ids(), vec![&cached_trade_id, &fill.trade_id]);
}

#[tokio::test]
async fn test_working_order_with_new_fills_updates_correctly() {
    // Tests incremental fill reconciliation for a working order
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let venue_order_id = VenueOrderId::from("V-WORKING-001");
    let client_order_id = ClientOrderId::from("O-WORKING-001");

    // Start with order already partially filled (3 of 10)
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .quantity(Quantity::from("10.0"))
        .price(Price::from("3000.00"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    order.apply(submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    order.apply(accepted).unwrap();
    let first_fill = TestOrderEventStubs::filled(
        &order,
        &test_instrument(),
        Some(TradeId::from("T-PREV-001")),
        None,
        Some(Price::from("3000.00")),
        Some(Quantity::from("3.0")),
        None,
        None,
        None,
        None,
    );
    order.apply(first_fill).unwrap();
    ctx.add_order(order);
    ctx.cache
        .borrow_mut()
        .add_venue_order_id(&client_order_id, &venue_order_id, false)
        .unwrap();

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Venue reports 7 filled (was 3, now +4)
    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("7.0"),
    )
    .with_avg_px(dec!(3000.0));

    let new_fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-NEW-001"),
        OrderSide::Buy,
        Quantity::from("4.0"),
        Price::from("3000.00"),
        Money::from("0.40 USDT"),
        LiquiditySide::Maker,
        None,
        None,
        UnixNanos::from(2_000),
        UnixNanos::from(2_000),
        None,
    );

    mass_status.add_order_reports(vec![report]);
    mass_status.add_fill_reports(vec![new_fill]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let has_fills = result
        .events
        .iter()
        .any(|e| matches!(e, OrderEventAny::Filled(_)));
    assert!(has_fills, "Should generate fill events");

    let order = ctx.get_order(&client_order_id).expect("Order should exist");
    assert_eq!(
        order.filled_qty(),
        Quantity::from("7.0"),
        "Order should have updated filled qty"
    );
}

#[rstest]
#[case::orphan(false)]
#[case::with_order_status(true)]
#[tokio::test]
async fn test_mass_status_recovers_later_fill_for_fill_report_order(
    #[case] with_order_status: bool,
) {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());
    let venue_order_id = VenueOrderId::from("V-FILL-REPORT");
    let client_order_id = ClientOrderId::from("V-FILL-REPORT");
    let position_id = PositionId::from("P-FILL-REPORT");
    let position = create_test_position(
        &test_instrument(),
        position_id,
        OrderSide::Sell,
        "3.0",
        "3200.00",
    );
    ctx.add_position(&position);

    let first = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-FILL-1"),
        OrderSide::Buy,
        Quantity::from("1.0"),
        Price::from("3000.00"),
        Money::from("0.10 USDT"),
        LiquiditySide::Taker,
        None,
        Some(position_id),
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
        None,
    );

    ctx.exec_engine.borrow_mut().reconcile_fill_report(&first);
    assert_eq!(
        ctx.get_order(&client_order_id).unwrap().status(),
        OrderStatus::Filled
    );
    let mut second = first.clone();
    second.report_id = UUID4::new();
    second.trade_id = TradeId::from("T-FILL-2");
    second.last_qty = Quantity::from("2.0");
    second.last_px = Price::from("3100.00");
    second.commission = Money::from("0.25 USDT");
    second.ts_event = UnixNanos::from(2_000);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::from(3_000),
        None,
    );

    mass_status.add_fill_reports(vec![first.clone(), second.clone()]);

    if with_order_status {
        let status = OrderStatusReport::new(
            test_account_id(),
            instrument_id,
            None,
            venue_order_id,
            OrderSide::Buy.into(),
            OrderType::Market,
            TimeInForce::Ioc,
            OrderStatus::Filled,
            Quantity::from("3.0"),
            Quantity::from("3.0"),
            UnixNanos::from(1_000),
            UnixNanos::from(2_000),
            UnixNanos::from(3_000),
            None,
        )
        .with_avg_px(dec!(9200) / dec!(3));
        mass_status.add_order_reports(vec![status]);
    }

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);
    let order = ctx.get_order(&client_order_id).unwrap();
    let position = ctx.cache.borrow().position_owned(&position_id).unwrap();

    let fills: Vec<_> = result
        .events
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => {
                Some((fill.trade_id, fill.last_qty, fill.last_px, fill.commission))
            }
            _ => None,
        })
        .collect();

    let mut expected_fills = vec![];
    if !with_order_status {
        expected_fills.push((
            first.trade_id,
            first.last_qty,
            first.last_px,
            Some(first.commission),
        ));
    }

    expected_fills.push((
        second.trade_id,
        second.last_qty,
        second.last_px,
        Some(second.commission),
    ));
    assert_eq!(fills, expected_fills);
    assert_eq!(order.quantity(), Quantity::from("3.0"));
    assert_eq!(order.filled_qty(), Quantity::from("3.0"));
    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.trade_ids(), vec![&first.trade_id, &second.trade_id]);
    assert_eq!(position.quantity, Quantity::from("0.0"));
    assert_eq!(position.side, PositionSide::Flat);
    assert_eq!(position.avg_px_open, dec!(3200));
    assert_eq!(
        position.commissions(),
        vec![Money::from("2 USD"), Money::from("0.35 USDT")]
    );

    let replay = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);
    assert_eq!(replay.events, Vec::<OrderEventAny>::new());
    assert_eq!(
        serde_json::to_value(ctx.get_order(&client_order_id).unwrap()).unwrap(),
        serde_json::to_value(order).unwrap()
    );
    assert_eq!(
        serde_json::to_value(ctx.cache.borrow().position_owned(&position_id).unwrap()).unwrap(),
        serde_json::to_value(position).unwrap()
    );
}

#[tokio::test]
async fn test_filtered_client_order_ids_skips_matching_orders() {
    // Orders in filtered_client_order_ids should be skipped during reconciliation
    let filtered_id = ClientOrderId::from("O-FILTERED-001");

    let config = ExecutionManagerConfig {
        filtered_client_order_ids: IndexSet::from([filtered_id]),
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Add an order report that should be filtered
    let report = create_order_report(
        Some(filtered_id),
        VenueOrderId::from("V-FILTERED-001"),
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("10.0"),
        Quantity::from("0.0"),
    );
    mass_status.add_order_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // No events should be generated for filtered order
    assert!(
        result.events.is_empty(),
        "Filtered order should not generate events"
    );

    // Order should not be in cache
    assert!(
        ctx.get_order(&filtered_id).is_none(),
        "Filtered order should not be added to cache"
    );
}

#[tokio::test]
async fn test_reconciliation_instrument_ids_filters_other_instruments() {
    // Only instruments in reconciliation_instrument_ids should be reconciled
    let included_instrument = test_instrument_id();

    let config = ExecutionManagerConfig {
        reconciliation_instrument_ids: IndexSet::from([included_instrument]),
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    // Add a second instrument that's NOT in the filter list
    let excluded_instrument = test_instrument2();
    let excluded_instrument_id = test_instrument_id2();
    ctx.add_instrument(excluded_instrument);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Add order for included instrument
    let included_report = create_order_report(
        Some(ClientOrderId::from("O-INCLUDED-001")),
        VenueOrderId::from("V-INCLUDED-001"),
        included_instrument,
        OrderStatus::Accepted,
        Quantity::from("5.0"),
        Quantity::from("0.0"),
    );

    // Add order for excluded instrument
    let excluded_report = create_order_report(
        Some(ClientOrderId::from("O-EXCLUDED-001")),
        VenueOrderId::from("V-EXCLUDED-001"),
        excluded_instrument_id,
        OrderStatus::Accepted,
        Quantity::from("5.0"),
        Quantity::from("0.0"),
    );

    mass_status.add_order_reports(vec![included_report, excluded_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Only included instrument should have generated events
    let included_order = ctx.get_order(&ClientOrderId::from("O-INCLUDED-001"));
    let excluded_order = ctx.get_order(&ClientOrderId::from("O-EXCLUDED-001"));

    assert!(
        included_order.is_some(),
        "Included instrument order should be reconciled"
    );
    assert!(
        excluded_order.is_none(),
        "Excluded instrument order should NOT be reconciled"
    );

    // Events should only be for included instrument
    let has_excluded_events = result.events.iter().any(|e| match e {
        OrderEventAny::Initialized(init) => init.instrument_id == excluded_instrument_id,
        OrderEventAny::Accepted(acc) => acc.instrument_id == excluded_instrument_id,
        _ => false,
    });

    assert!(
        !has_excluded_events,
        "No events should be generated for excluded instrument"
    );
}

#[tokio::test]
async fn test_reconcile_mass_status_publishes_raw_reports_for_capture() {
    // Live mass-status reconciliation bypasses the per-report engine entry
    // points, so the raw venue inputs must be published from this path or the
    // event store has no record of them for forensic replay.
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let order_topic = MessagingSwitchboard::reconciliation_raw_order_status_report_topic();
    let fill_topic = MessagingSwitchboard::reconciliation_raw_fill_report_topic();
    let position_topic = MessagingSwitchboard::reconciliation_raw_position_status_report_topic();

    let (order_handler, order_saver) = get_any_saving_handler::<OrderStatusReport>(None);
    let (fill_handler, fill_saver) = get_any_saving_handler::<FillReport>(None);
    let (position_handler, position_saver) = get_any_saving_handler::<PositionStatusReport>(None);

    let order_pattern: msgbus::MStr<msgbus::Pattern> = order_topic.into();
    let fill_pattern: msgbus::MStr<msgbus::Pattern> = fill_topic.into();
    let position_pattern: msgbus::MStr<msgbus::Pattern> = position_topic.into();
    msgbus::subscribe_any(order_pattern, order_handler.clone(), None);
    msgbus::subscribe_any(fill_pattern, fill_handler.clone(), None);
    msgbus::subscribe_any(position_pattern, position_handler.clone(), None);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let order_report = create_order_report(
        None,
        VenueOrderId::from("V-RAW-CAPTURE"),
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0"),
    );
    mass_status.add_order_reports(vec![order_report.clone()]);

    let fill_report = FillReport::new(
        test_account_id(),
        instrument_id,
        VenueOrderId::from("V-RAW-CAPTURE"),
        TradeId::from("T-RAW-CAPTURE"),
        OrderSide::Buy,
        Quantity::from("1.0"),
        Price::from("3000.00"),
        Money::new(0.0, Currency::USDT()),
        LiquiditySide::Maker,
        None,
        None,
        UnixNanos::from(2_000_000),
        UnixNanos::from(2_000_000),
        None,
    );

    mass_status.add_fill_reports(vec![fill_report.clone()]);

    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("1.0"),
        UnixNanos::from(3_000_000),
        UnixNanos::from(3_000_000),
        None,
        None,
        Some(dec!(3000.0)),
    );

    mass_status.add_position_reports(vec![position_report.clone()]);

    let _ = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    msgbus::unsubscribe_any(order_pattern, &order_handler);
    msgbus::unsubscribe_any(fill_pattern, &fill_handler);
    msgbus::unsubscribe_any(position_pattern, &position_handler);

    let orders = order_saver.get_messages();
    assert_eq!(
        orders.len(),
        1,
        "raw OrderStatusReport must be published once"
    );
    assert_eq!(orders[0], order_report);

    let fills = fill_saver.get_messages();
    assert_eq!(fills.len(), 1, "raw FillReport must be published once");
    assert_eq!(fills[0], fill_report);

    let positions = position_saver.get_messages();
    assert_eq!(
        positions.len(),
        1,
        "raw PositionStatusReport must be published once",
    );
    assert_eq!(positions[0], position_report);
}

#[tokio::test]
async fn test_reconcile_mass_status_does_not_capture_synthetic_reports() {
    // The raw publish must happen BEFORE adjust_mass_status_fills, which can
    // synthesize replacement order/fill reports via
    // process_mass_status_for_reconciliation. Forensic replay must see only
    // the venue-supplied raw inputs; synthetic reports are an internal
    // reconstruction step and must never appear on `reconciliation.raw.*`.
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let order_topic = MessagingSwitchboard::reconciliation_raw_order_status_report_topic();
    let fill_topic = MessagingSwitchboard::reconciliation_raw_fill_report_topic();
    let order_pattern: msgbus::MStr<msgbus::Pattern> = order_topic.into();
    let fill_pattern: msgbus::MStr<msgbus::Pattern> = fill_topic.into();
    let (order_handler, order_saver) = get_any_saving_handler::<OrderStatusReport>(None);
    let (fill_handler, fill_saver) = get_any_saving_handler::<FillReport>(None);
    msgbus::subscribe_any(order_pattern, order_handler.clone(), None);
    msgbus::subscribe_any(fill_pattern, fill_handler.clone(), None);

    // Build a mass status that triggers AddSyntheticOpening: simulated qty
    // (0.4 from the single fill) does not match the venue position (Long 1.0),
    // so the adjustment step inserts a synthetic Buy 0.6 opening fill under a
    // new `S-...` venue_order_id.
    let venue_order_id = VenueOrderId::from("V-SYN-RAW");

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let order_report = create_order_report(
        None,
        venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("1.0"),
        Quantity::from("0.4"),
    );
    mass_status.add_order_reports(vec![order_report.clone()]);

    let fill_report = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-SYN-RAW"),
        OrderSide::Buy,
        Quantity::from("0.4"),
        Price::from("3000.00"),
        Money::new(0.0, Currency::USDT()),
        LiquiditySide::Maker,
        None,
        None,
        UnixNanos::from(2_000_000),
        UnixNanos::from(2_000_000),
        None,
    );

    mass_status.add_fill_reports(vec![fill_report.clone()]);

    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("1.0"),
        UnixNanos::from(3_000_000),
        UnixNanos::from(3_000_000),
        None,
        None, // netting mode triggers adjust_mass_status_fills
        Some(dec!(3000.0)),
    );

    mass_status.add_position_reports(vec![position_report]);

    let _ = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    msgbus::unsubscribe_any(order_pattern, &order_handler);
    msgbus::unsubscribe_any(fill_pattern, &fill_handler);

    let orders = order_saver.get_messages();
    assert_eq!(
        orders.len(),
        1,
        "only the raw venue OrderStatusReport must be captured; \
         synthetic reports from adjust_mass_status_fills must not reach \
         the raw topic",
    );
    assert_eq!(orders[0], order_report);
    assert_eq!(
        orders[0].venue_order_id, venue_order_id,
        "captured venue_order_id must match the original raw input, not a synthetic `S-` id",
    );

    let fills = fill_saver.get_messages();
    assert_eq!(
        fills.len(),
        1,
        "only the raw venue FillReport must be captured; the synthetic \
         opening fill inserted by adjustment must not appear on the raw topic",
    );
    assert_eq!(fills[0], fill_report);
    assert_eq!(
        fills[0].trade_id,
        TradeId::from("T-SYN-RAW"),
        "captured trade_id must match the original raw input, not a synthetic `S-` id",
    );
}

#[rstest]
#[case::accepted(OrderStatus::Accepted, "0", true)]
#[case::partially_filled(OrderStatus::PartiallyFilled, "2", true)]
#[case::unchanged(OrderStatus::Accepted, "0", false)]
#[tokio::test]
async fn test_reconcile_pending_update_working_report(
    #[case] status: OrderStatus,
    #[case] filled_qty: &str,
    #[case] changed: bool,
) {
    let mut ctx = TestContext::new();
    ctx.add_instrument(test_instrument());
    let cid = ClientOrderId::from("O-PENDING-AMEND");
    let vid = VenueOrderId::from("V-PENDING-AMEND");
    let order = create_pending_update_order(
        "O-PENDING-AMEND",
        test_instrument_id(),
        OrderSide::Buy,
        "1",
        "3000.00",
        vid,
    );
    ctx.add_order(order);
    let mut report = create_order_report(
        Some(cid),
        vid,
        test_instrument_id(),
        status,
        Quantity::from(if changed { "3" } else { "1" }),
        Quantity::from(filled_qty),
    );

    if changed {
        report.price = Some(Price::from("3100.00"));
    }

    if status == OrderStatus::PartiallyFilled {
        report.avg_px = Some(dec!(3050));
    }

    let mass = create_mass_status(vec![report], vec![]);
    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass, &ctx.exec_engine);
    let after = ctx.get_order(&cid).unwrap();

    if changed {
        assert_eq!(
            result.events.len(),
            if status == OrderStatus::PartiallyFilled {
                2
            } else {
                1
            }
        );
        assert!(matches!(result.events[0], OrderEventAny::Updated(_)));

        if status == OrderStatus::PartiallyFilled {
            assert!(matches!(result.events[1], OrderEventAny::Filled(_)));
        }

        assert_eq!(after.status(), status);
        assert_eq!(after.quantity(), Quantity::from("3"));
        assert_eq!(after.price(), Some(Price::from("3100.00")));
    } else {
        assert!(result.events.is_empty());
        assert_eq!(after.status(), OrderStatus::PendingUpdate);
        assert_eq!(after.quantity(), Quantity::from("1"));
        assert_eq!(after.price(), Some(Price::from("3000.00")));
    }

    assert_eq!(after.filled_qty(), Quantity::from(filled_qty));
}

#[rstest]
#[case::working(OrderStatus::Accepted)]
#[case::triggered(OrderStatus::Triggered)]
#[tokio::test]
async fn test_reconcile_pending_update_trigger_price(#[case] status: OrderStatus) {
    let mut ctx = TestContext::new();
    ctx.add_instrument(test_instrument());
    let cid = ClientOrderId::from("O-TRIGGER-AMEND");
    let vid = VenueOrderId::from("V-TRIGGER-AMEND");
    let mut order = OrderTestBuilder::new(OrderType::StopLimit)
        .instrument_id(test_instrument_id())
        .client_order_id(cid)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1"))
        .price(Price::from("3100.00"))
        .trigger_price(Price::from("3200.00"))
        .build();
    apply_submitted_and_accepted(&mut order, vid);
    let pending = OrderPendingUpdateSpec::builder()
        .trader_id(order.trader_id())
        .strategy_id(order.strategy_id())
        .instrument_id(order.instrument_id())
        .client_order_id(cid)
        .account_id(test_account_id())
        .venue_order_id(vid)
        .build();
    order.apply(OrderEventAny::PendingUpdate(pending)).unwrap();
    ctx.add_order(order);
    let mut report = create_order_report(
        Some(cid),
        vid,
        test_instrument_id(),
        status,
        Quantity::from("1"),
        Quantity::from(0),
    );
    report.order_type = OrderType::StopLimit;
    report.price = Some(Price::from("3100.00"));
    report.trigger_price = Some(Price::from("3250.00"));
    report.ts_triggered = Some(report.ts_last);
    let mass = create_mass_status(vec![report], vec![]);
    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass, &ctx.exec_engine);
    assert_eq!(
        result.events.len(),
        if status == OrderStatus::Triggered {
            2
        } else {
            1
        }
    );

    let OrderEventAny::Updated(updated) = &result.events[0] else {
        panic!("expected amendment before trigger, was {:?}", result.events);
    };

    assert_eq!(updated.trigger_price, Some(Price::from("3250.00")));

    if status == OrderStatus::Triggered {
        assert!(matches!(result.events[1], OrderEventAny::Triggered(_)));
    }

    let after = ctx.get_order(&cid).unwrap();
    assert_eq!(after.status(), status);
    assert_eq!(after.quantity(), Quantity::from("1"));
    assert_eq!(after.price(), Some(Price::from("3100.00")));
    assert_eq!(after.trigger_price(), Some(Price::from("3250.00")));
    assert_eq!(after.filled_qty(), Quantity::from(0));
}

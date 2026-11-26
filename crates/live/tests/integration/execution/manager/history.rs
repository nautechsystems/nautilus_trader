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

use nautilus_model::events::order::spec::OrderEmulatedSpec;

use super::*;

#[rstest]
#[case(OmsType::Netting, true, true, true)]
#[case(OmsType::Netting, true, false, true)]
#[case(OmsType::Netting, false, true, true)]
#[case(OmsType::Netting, false, false, true)]
#[case(OmsType::Hedging, true, true, true)]
#[case(OmsType::Hedging, true, false, true)]
#[case(OmsType::Hedging, false, true, true)]
#[case(OmsType::Hedging, false, false, true)]
#[case(OmsType::Netting, true, false, false)]
#[case(OmsType::Netting, false, false, false)]
#[case(OmsType::Hedging, true, false, false)]
#[case(OmsType::Hedging, false, false, false)]
#[tokio::test]
async fn test_retained_fill_projects_missing_order_without_reapplying(
    #[case] oms_type: OmsType,
    #[case] generate_missing_orders: bool,
    #[case] include_fill_report: bool,
    #[case] include_client_order_id: bool,
) {
    let config = ExecutionManagerConfig {
        generate_missing_orders,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let strategy_id = StrategyId::from("STRATEGY-001");
    let client_order_id = ClientOrderId::from("O-RETAINED-001");
    let venue_order_id = VenueOrderId::from("V-RETAINED-001");
    let trade_id = TradeId::from("T-RETAINED-001");

    let position_id = match oms_type {
        OmsType::Hedging => PositionId::from("P-RETAINED-001"),
        _ => PositionId::new(format!("{instrument_id}-{strategy_id}")),
    };

    ctx.add_instrument(instrument.clone());
    ctx.manager
        .claim_external_orders(instrument_id, strategy_id)
        .unwrap();
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(strategy_id, oms_type);

    let mut restored_order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .strategy_id(strategy_id)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .price(Price::from("3000.00"))
        .build();
    apply_submitted_and_accepted(&mut restored_order, venue_order_id);
    let restored_fill = TestOrderEventStubs::filled(
        &restored_order,
        &instrument,
        Some(trade_id),
        Some(position_id),
        Some(Price::from("3000.00")),
        Some(Quantity::from("1.000")),
        Some(LiquiditySide::Maker),
        Some(Money::from("0.50 USDT")),
        Some(UnixNanos::from(1_000_000)),
        Some(test_account_id()),
    );
    let position = Position::new(&instrument, restored_fill.into());
    ctx.cache
        .borrow_mut()
        .add_position(&position, oms_type)
        .unwrap();

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    mass_status.set_report_window(Some(UnixNanos::from(2_000_000)), true);
    let mut order_report = create_order_report(
        include_client_order_id.then_some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("1.000"),
        Quantity::from("1.000"),
    )
    .with_avg_px(dec!(3000.0));
    let venue_position_id = (oms_type == OmsType::Hedging).then_some(position_id);
    if let Some(position_id) = venue_position_id {
        order_report = order_report.with_venue_position_id(position_id);
    }

    let fill_report = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        trade_id,
        OrderSide::Buy,
        Quantity::from("1.000"),
        Price::from("3000.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Maker,
        Some(client_order_id),
        venue_position_id,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );

    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("1.000"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        venue_position_id,
        Some(dec!(3000.00)),
    );

    mass_status.add_order_reports(vec![order_report]);
    if include_fill_report {
        mass_status.add_fill_reports(vec![fill_report]);
    }

    mass_status.add_position_reports(vec![position_report]);

    ctx.manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let cache = ctx.cache.borrow();

    let reconciled_order_id = if include_client_order_id {
        client_order_id
    } else {
        ClientOrderId::from(venue_order_id.as_str())
    };

    let order = cache.order(&reconciled_order_id).unwrap();
    let position = cache.position(&position_id).unwrap();

    assert_eq!(cache.oms_type(&position_id), Some(oms_type));
    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.filled_qty(), Quantity::from("1.000"));
    assert_eq!(position.quantity, Quantity::from("1.000"));
    assert_eq!(position.realized_pnl, Some(Money::from("-0.50 USDT")));
    assert_eq!(position.commissions(), vec![Money::from("0.50 USDT")]);
    assert_eq!(position.trade_ids.len(), 1);
    assert!(position.trade_ids.contains(&trade_id));
}

#[rstest]
#[case::unbounded_missing_orders(true, false)]
#[case::unbounded_no_missing_orders(false, false)]
#[case::bounded_missing_orders(true, true)]
#[case::bounded_no_missing_orders(false, true)]
#[tokio::test]
async fn test_inferred_delta_for_retained_order_applies_new_economics(
    #[case] generate_missing_orders: bool,
    #[case] bounded: bool,
) {
    let config = ExecutionManagerConfig {
        generate_missing_orders,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let strategy_id = StrategyId::from("STRATEGY-001");
    let client_order_id = ClientOrderId::from("O-RETAINED-PARTIAL");
    let venue_order_id = VenueOrderId::from("V-RETAINED-PARTIAL");
    let known_trade_id = TradeId::from("T-RETAINED-PARTIAL");
    let position_id = PositionId::new(format!("{instrument_id}-{strategy_id}"));

    ctx.add_instrument(instrument.clone());
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(strategy_id, OmsType::Netting);

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .strategy_id(strategy_id)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("10.000"))
        .price(Price::from("3000.00"))
        .build();
    apply_submitted_and_accepted(&mut order, venue_order_id);
    let known_fill = TestOrderEventStubs::filled(
        &order,
        &instrument,
        Some(known_trade_id),
        Some(position_id),
        Some(Price::from("3000.00")),
        Some(Quantity::from("5.000")),
        Some(LiquiditySide::Maker),
        Some(Money::from("0.50 USDT")),
        Some(UnixNanos::from(1_000_000)),
        Some(test_account_id()),
    );
    order.apply(known_fill.clone()).unwrap();
    let position = Position::new(&instrument, known_fill.into());
    ctx.add_order(order);
    ctx.cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
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
        OrderStatus::Filled,
        Quantity::from("10.000"),
        Quantity::from("10.000"),
    )
    .with_avg_px(dec!(3000.0));

    if bounded {
        mass_status.set_report_window(Some(UnixNanos::from(500_000)), true);
        mass_status.add_position_reports(vec![PositionStatusReport::new(
            test_account_id(),
            instrument_id,
            PositionSide::Long,
            Quantity::from("10.000"),
            UnixNanos::from(2_000_000),
            UnixNanos::from(2_000_000),
            None,
            None,
            Some(dec!(3000.00)),
        )]);
    }

    mass_status.add_order_reports(vec![report]);

    ctx.manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let cache = ctx.cache.borrow();
    let order = cache.order(&client_order_id).unwrap();
    let position = cache.position(&position_id).unwrap();

    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.filled_qty(), Quantity::from("10.000"));
    assert_eq!(position.quantity, Quantity::from("10.000"));
    assert_eq!(position.realized_pnl, Some(Money::from("-0.50 USDT")));
    assert_eq!(position.commissions(), vec![Money::from("0.50 USDT")]);
    assert_eq!(position.trade_ids.len(), 2);
    assert!(position.trade_ids.contains(&known_trade_id));
}

#[tokio::test]
async fn test_missing_venue_order_id_collision_is_scoped_by_instrument() {
    let mut ctx = TestContext::new();
    let retained_instrument = test_instrument();
    let retained_instrument_id = retained_instrument.id();
    let new_instrument = test_instrument2();
    let new_instrument_id = new_instrument.id();
    let strategy_id = StrategyId::from("STRATEGY-001");
    let venue_order_id = VenueOrderId::from("V-COLLISION");
    let retained_position_id = PositionId::new(format!("{retained_instrument_id}-{strategy_id}"));

    ctx.add_instrument(retained_instrument.clone());
    ctx.add_instrument(new_instrument);
    ctx.manager
        .claim_external_orders(new_instrument_id, strategy_id)
        .unwrap();
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(strategy_id, OmsType::Netting);

    let mut retained_order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(ClientOrderId::from("O-RETAINED-COLLISION"))
        .strategy_id(strategy_id)
        .instrument_id(retained_instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .price(Price::from("3000.00"))
        .build();
    apply_submitted_and_accepted(&mut retained_order, venue_order_id);
    let retained_fill = TestOrderEventStubs::filled(
        &retained_order,
        &retained_instrument,
        Some(TradeId::from("T-RETAINED-COLLISION")),
        Some(retained_position_id),
        Some(Price::from("3000.00")),
        Some(Quantity::from("1.000")),
        Some(LiquiditySide::Maker),
        Some(Money::from("0.50 USDT")),
        Some(UnixNanos::from(1_000_000)),
        Some(test_account_id()),
    );
    let retained_position = Position::new(&retained_instrument, retained_fill.into());
    ctx.cache
        .borrow_mut()
        .add_position(&retained_position, OmsType::Netting)
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
        new_instrument_id,
        OrderStatus::Filled,
        Quantity::from("1.000"),
        Quantity::from("1.000"),
    )
    .with_avg_px(dec!(3000.0));
    mass_status.add_order_reports(vec![report]);

    ctx.manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let new_position_id = PositionId::new(format!("{new_instrument_id}-{strategy_id}"));
    let cache = ctx.cache.borrow();
    let order = cache
        .order(&ClientOrderId::from(venue_order_id.as_str()))
        .unwrap();
    let position = cache.position(&new_position_id).unwrap();

    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(position.quantity, Quantity::from("1.000"));
    assert_eq!(position.trade_ids.len(), 1);
}

#[rstest]
#[case::unbounded_missing_orders(true, false)]
#[case::unbounded_no_missing_orders(false, false)]
#[case::bounded_missing_orders(true, true)]
#[case::bounded_no_missing_orders(false, true)]
#[tokio::test]
async fn test_partially_known_fills_apply_only_new_economics(
    #[case] generate_missing_orders: bool,
    #[case] bounded: bool,
) {
    let config = ExecutionManagerConfig {
        generate_missing_orders,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let strategy_id = StrategyId::from("STRATEGY-001");
    let client_order_id = ClientOrderId::from("O-RETAINED-002");
    let venue_order_id = VenueOrderId::from("V-RETAINED-002");
    let known_trade_id = TradeId::from("T-RETAINED-002-A");
    let new_trade_id = TradeId::from("T-RETAINED-002-B");
    let position_id = PositionId::new(format!("{instrument_id}-{strategy_id}"));

    ctx.add_instrument(instrument.clone());
    ctx.manager
        .claim_external_orders(instrument_id, strategy_id)
        .unwrap();
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(strategy_id, OmsType::Netting);

    let mut restored_order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .strategy_id(strategy_id)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("2.000"))
        .price(Price::from("3000.00"))
        .build();
    apply_submitted_and_accepted(&mut restored_order, venue_order_id);
    let known_fill = TestOrderEventStubs::filled(
        &restored_order,
        &instrument,
        Some(known_trade_id),
        Some(position_id),
        Some(Price::from("3000.00")),
        Some(Quantity::from("2.000")),
        Some(LiquiditySide::Maker),
        Some(Money::from("1.00 USDT")),
        Some(UnixNanos::from(1_000_000)),
        Some(test_account_id()),
    );
    let position = Position::new(&instrument, known_fill.into());
    ctx.cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    if bounded {
        mass_status.set_report_window(Some(UnixNanos::from(500_000)), true);
    }

    let order_report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("3.000"),
        Quantity::from("3.000"),
    )
    .with_avg_px(dec!(3033.333333));

    let known_fill_report = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        known_trade_id,
        OrderSide::Buy,
        Quantity::from("2.000"),
        Price::from("3000.00"),
        Money::from("1.00 USDT"),
        LiquiditySide::Maker,
        Some(client_order_id),
        None,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );

    let new_fill_report = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        new_trade_id,
        OrderSide::Buy,
        Quantity::from("1.000"),
        Price::from("3100.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Taker,
        Some(client_order_id),
        None,
        UnixNanos::from(2_000_000),
        UnixNanos::from(2_000_000),
        None,
    );

    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("3.000"),
        UnixNanos::from(2_000_000),
        UnixNanos::from(2_000_000),
        None,
        None,
        Some(dec!(3033.333333)),
    );

    mass_status.add_order_reports(vec![order_report]);
    mass_status.add_fill_reports(vec![known_fill_report, new_fill_report]);
    mass_status.add_position_reports(vec![position_report]);

    ctx.manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let cache = ctx.cache.borrow();
    let order = cache.order(&client_order_id).unwrap();
    let position = cache.position(&position_id).unwrap();

    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.filled_qty(), Quantity::from("3.000"));
    assert_eq!(position.quantity, Quantity::from("3.000"));
    assert_eq!(position.realized_pnl, Some(Money::from("-1.50 USDT")));
    assert_eq!(position.commissions(), vec![Money::from("1.50 USDT")]);
    assert_eq!(position.trade_ids.len(), 2);
    assert!(position.trade_ids.contains(&known_trade_id));
    assert!(position.trade_ids.contains(&new_trade_id));
}

#[rstest]
#[case(true)]
#[case(false)]
#[tokio::test]
async fn test_partial_window_known_fill_does_not_reapply_economics(
    #[case] generate_missing_orders: bool,
) {
    let config = ExecutionManagerConfig {
        generate_missing_orders,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let strategy_id = StrategyId::from("STRATEGY-001");
    let opening_order_id = ClientOrderId::from("O-PARTIAL-WINDOW-OPEN");
    let closing_order_id = ClientOrderId::from("O-PARTIAL-WINDOW-CLOSE");
    let closing_venue_order_id = VenueOrderId::from("V-PARTIAL-WINDOW-CLOSE");
    let opening_trade_id = TradeId::from("T-PARTIAL-WINDOW-OPEN");
    let closing_trade_id = TradeId::from("T-PARTIAL-WINDOW-CLOSE");
    let position_id = PositionId::new(format!("{instrument_id}-{strategy_id}"));

    ctx.add_instrument(instrument.clone());
    ctx.manager
        .claim_external_orders(instrument_id, strategy_id)
        .unwrap();
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(strategy_id, OmsType::Netting);

    let mut opening_order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(opening_order_id)
        .strategy_id(strategy_id)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("5.000"))
        .price(Price::from("3000.00"))
        .build();
    apply_submitted_and_accepted(
        &mut opening_order,
        VenueOrderId::from("V-PARTIAL-WINDOW-OPEN"),
    );
    let opening_fill = TestOrderEventStubs::filled(
        &opening_order,
        &instrument,
        Some(opening_trade_id),
        Some(position_id),
        Some(Price::from("3000.00")),
        Some(Quantity::from("5.000")),
        Some(LiquiditySide::Maker),
        Some(Money::from("2.50 USDT")),
        Some(UnixNanos::from(1_000_000)),
        Some(test_account_id()),
    );
    let mut position = Position::new(&instrument, opening_fill.into());

    let mut closing_order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(closing_order_id)
        .strategy_id(strategy_id)
        .instrument_id(instrument_id)
        .side(OrderSide::Sell)
        .quantity(Quantity::from("2.000"))
        .price(Price::from("3100.00"))
        .build();
    apply_submitted_and_accepted(&mut closing_order, closing_venue_order_id);
    let closing_fill = TestOrderEventStubs::filled(
        &closing_order,
        &instrument,
        Some(closing_trade_id),
        Some(position_id),
        Some(Price::from("3100.00")),
        Some(Quantity::from("2.000")),
        Some(LiquiditySide::Taker),
        Some(Money::from("1.00 USDT")),
        Some(UnixNanos::from(2_000_000)),
        Some(test_account_id()),
    );
    position.apply(&closing_fill.into());
    ctx.cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let order_report = create_order_report_for_side(
        Some(closing_order_id),
        closing_venue_order_id,
        instrument_id,
        OrderSide::Sell,
        OrderStatus::Filled,
        Quantity::from("2.000"),
        Quantity::from("2.000"),
    )
    .with_avg_px(dec!(3100.0));

    let fill_report = FillReport::new(
        test_account_id(),
        instrument_id,
        closing_venue_order_id,
        closing_trade_id,
        OrderSide::Sell,
        Quantity::from("2.000"),
        Price::from("3100.00"),
        Money::from("1.00 USDT"),
        LiquiditySide::Taker,
        Some(closing_order_id),
        None,
        UnixNanos::from(2_000_000),
        UnixNanos::from(2_000_000),
        None,
    );

    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("3.000"),
        UnixNanos::from(2_000_000),
        UnixNanos::from(2_000_000),
        None,
        None,
        Some(dec!(3000.00)),
    );

    mass_status.add_order_reports(vec![order_report]);
    mass_status.add_fill_reports(vec![fill_report]);
    mass_status.add_position_reports(vec![position_report]);

    ctx.manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let cache = ctx.cache.borrow();
    let order = cache.order(&closing_order_id).unwrap();
    let position = cache.position(&position_id).unwrap();

    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(position.quantity, Quantity::from("3.000"));
    assert_eq!(position.realized_pnl, Some(Money::from("196.50 USDT")));
    assert_eq!(position.commissions(), vec![Money::from("3.50 USDT")]);
    assert_eq!(position.trade_ids.len(), 2);
    assert!(position.trade_ids.contains(&opening_trade_id));
    assert!(position.trade_ids.contains(&closing_trade_id));
}

#[tokio::test]
async fn test_split_lighter_reduce_only_lifecycle_preserves_explicit_flat() {
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let strategy_id = StrategyId::from("EXTERNAL");
    let venue_order_id = VenueOrderId::from("V-LIGHTER-SPLIT-CLOSE");
    let trade_id = TradeId::from("T-LIGHTER-SPLIT-CLOSE");
    let cutoff = UnixNanos::from(1_000_000_000_000);
    let close_ts = UnixNanos::from(1_000_001_000_000);

    ctx.add_instrument(instrument);
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(strategy_id, OmsType::Netting);

    let (portfolio_handler, portfolio_events) =
        get_typed_into_message_saving_handler::<OrderEventAny>(None);
    let portfolio_endpoint = MessagingSwitchboard::portfolio_update_order();
    msgbus::register_order_event_endpoint(portfolio_endpoint, portfolio_handler);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        close_ts,
        Some(UUID4::new()),
    );

    mass_status.set_report_window(Some(cutoff), true);
    let order_report = OrderStatusReport::new(
        test_account_id(),
        instrument_id,
        None,
        venue_order_id,
        OrderSide::Sell.into(),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Filled,
        Quantity::from("1.000"),
        Quantity::from("1.000"),
        close_ts,
        close_ts,
        close_ts,
        None,
    )
    .with_price(Price::from("3000.00"))
    .with_avg_px(dec!(3000.00))
    .with_reduce_only(true);

    let fill_report = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        trade_id,
        OrderSide::Sell,
        Quantity::from("1.000"),
        Price::from("3000.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Taker,
        None,
        None,
        close_ts,
        close_ts,
        None,
    );

    let flat_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Flat,
        Quantity::from("0"),
        close_ts,
        close_ts,
        None,
        None,
        None,
    );

    mass_status.add_order_reports(vec![order_report]);
    mass_status.add_fill_reports(vec![fill_report]);
    mass_status.add_position_reports(vec![flat_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    msgbus::deregister_any(portfolio_endpoint);

    assert_eq!(cutoff, UnixNanos::from(1_000_000_000_000));
    assert_eq!(close_ts, UnixNanos::from(1_000_001_000_000));
    assert_eq!(result.events.len(), 2);

    let OrderEventAny::Accepted(accepted) = &result.events[0] else {
        panic!("Expected Accepted event, was {:?}", result.events[0]);
    };

    let OrderEventAny::Filled(filled) = &result.events[1] else {
        panic!("Expected Filled event, was {:?}", result.events[1]);
    };

    assert_eq!(accepted.strategy_id, strategy_id);
    assert_eq!(accepted.instrument_id, instrument_id);
    assert_eq!(accepted.client_order_id.as_str(), venue_order_id.as_str());
    assert_eq!(accepted.venue_order_id, venue_order_id);
    assert_eq!(accepted.account_id, test_account_id());
    assert_eq!(accepted.ts_event, close_ts);
    assert_eq!(filled.strategy_id, strategy_id);
    assert_eq!(filled.instrument_id, instrument_id);
    assert_eq!(filled.client_order_id.as_str(), venue_order_id.as_str());
    assert_eq!(filled.venue_order_id, venue_order_id);
    assert_eq!(filled.account_id, test_account_id());
    assert_eq!(filled.trade_id, trade_id);
    assert_eq!(filled.position_id, None);
    assert_eq!(filled.order_side, OrderSide::Sell);
    assert_eq!(filled.order_type, OrderType::Limit);
    assert_eq!(filled.last_qty, Quantity::from("1.000"));
    assert_eq!(filled.last_px, Price::from("3000.00"));
    assert_eq!(filled.commission, Some(Money::from("0.50 USDT")));
    assert_eq!(filled.liquidity_side, LiquiditySide::Taker);
    assert_eq!(filled.ts_event, close_ts);
    assert_eq!(result.external_orders.len(), 1);
    assert_eq!(
        result.external_orders[0].client_order_id.as_str(),
        venue_order_id.as_str()
    );
    assert_eq!(result.external_orders[0].venue_order_id, venue_order_id);
    assert_eq!(result.external_orders[0].instrument_id, instrument_id);
    assert_eq!(result.external_orders[0].strategy_id, strategy_id);
    let order = ctx
        .cache
        .borrow()
        .order_owned(&ClientOrderId::from(venue_order_id.as_str()))
        .expect("reconciled terminal order");
    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.filled_qty(), Quantity::from("1.000"));
    assert!(order.is_reduce_only());
    assert_eq!(
        ctx.cache
            .borrow()
            .orders(None, None, None, None, None)
            .len(),
        1
    );
    assert_eq!(
        ctx.cache
            .borrow()
            .positions(None, None, None, None, None)
            .len(),
        0
    );
    let portfolio_events = portfolio_events.get_messages();
    assert_eq!(portfolio_events.len(), 2);
    assert!(matches!(portfolio_events[0], OrderEventAny::Accepted(_)));
    assert!(matches!(portfolio_events[1], OrderEventAny::Filled(_)));
    assert!(result.unresolved_positions.is_empty());
}

#[tokio::test]
async fn test_bounded_complete_lifecycle_applies_beside_split_close() {
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let strategy_id = StrategyId::from("EXTERNAL");
    let opening_venue_order_id = VenueOrderId::from("V-INSIDE-OPEN");
    let closing_venue_order_id = VenueOrderId::from("V-INSIDE-CLOSE");
    let split_venue_order_id = VenueOrderId::from("V-SPLIT-CLOSE");
    let opening_trade_id = TradeId::from("T-INSIDE-OPEN-1");
    let opening_trade_id_2 = TradeId::from("T-INSIDE-OPEN-2");
    let closing_trade_id = TradeId::from("T-INSIDE-CLOSE");
    let split_trade_id = TradeId::from("T-SPLIT-CLOSE");
    let cutoff = UnixNanos::from(1_000_000_000_000);
    let opening_ts = UnixNanos::from(1_000_001_000_000);
    let closing_ts = UnixNanos::from(1_000_002_000_000);
    let split_ts = UnixNanos::from(1_000_003_000_000);

    ctx.add_instrument(instrument);
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(strategy_id, OmsType::Netting);

    let (portfolio_handler, portfolio_events) =
        get_typed_into_message_saving_handler::<OrderEventAny>(None);
    let portfolio_endpoint = MessagingSwitchboard::portfolio_update_order();
    msgbus::register_order_event_endpoint(portfolio_endpoint, portfolio_handler);

    let (mut opening_order, opening_fill) = create_bounded_fill_lifecycle(
        instrument_id,
        opening_venue_order_id,
        opening_trade_id,
        OrderSide::Buy,
        "0.400",
        "3000.00",
        false,
        opening_ts,
    );
    opening_order.quantity = Quantity::from("1.000");
    opening_order.filled_qty = Quantity::from("1.000");
    let (_, opening_fill_2) = create_bounded_fill_lifecycle(
        instrument_id,
        opening_venue_order_id,
        opening_trade_id_2,
        OrderSide::Buy,
        "0.600",
        "3000.00",
        false,
        UnixNanos::from(opening_ts.as_u64() + 1),
    );
    let (closing_order, closing_fill) = create_bounded_fill_lifecycle(
        instrument_id,
        closing_venue_order_id,
        closing_trade_id,
        OrderSide::Sell,
        "1.000",
        "3100.00",
        true,
        closing_ts,
    );
    let (split_order, split_fill) = create_bounded_fill_lifecycle(
        instrument_id,
        split_venue_order_id,
        split_trade_id,
        OrderSide::Sell,
        "1.000",
        "3200.00",
        true,
        split_ts,
    );

    let flat_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Flat,
        Quantity::from("0"),
        split_ts,
        split_ts,
        None,
        None,
        None,
    );

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        split_ts,
        Some(UUID4::new()),
    );

    mass_status.set_report_window(Some(cutoff), true);
    mass_status.add_order_reports(vec![opening_order, closing_order, split_order]);
    mass_status.add_fill_reports(vec![opening_fill, opening_fill_2, closing_fill, split_fill]);
    mass_status.add_position_reports(vec![flat_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    msgbus::deregister_any(portfolio_endpoint);

    let actual_events: Vec<(&str, VenueOrderId)> = result
        .events
        .iter()
        .map(|event| match event {
            OrderEventAny::Accepted(event) => ("accepted", event.venue_order_id),
            OrderEventAny::Filled(event) => ("filled", event.venue_order_id),
            event => panic!("Unexpected reconciliation event: {event:?}"),
        })
        .collect();

    assert_eq!(
        actual_events,
        vec![
            ("accepted", opening_venue_order_id),
            ("filled", opening_venue_order_id),
            ("filled", opening_venue_order_id),
            ("accepted", closing_venue_order_id),
            ("filled", closing_venue_order_id),
            ("accepted", split_venue_order_id),
            ("filled", split_venue_order_id),
        ]
    );
    assert_eq!(result.external_orders.len(), 3);
    assert!(result.unresolved_positions.is_empty());

    let position_id = PositionId::new(format!("{instrument_id}-{strategy_id}"));
    let cache = ctx.cache.borrow();
    let position = cache
        .position(&position_id)
        .expect("closed inside lifecycle");
    assert!(position.is_closed());
    assert_eq!(position.quantity, Quantity::zero(3));
    assert_eq!(position.trade_ids.len(), 3);
    assert!(position.trade_ids.contains(&opening_trade_id));
    assert!(position.trade_ids.contains(&opening_trade_id_2));
    assert!(position.trade_ids.contains(&closing_trade_id));
    assert!(!position.trade_ids.contains(&split_trade_id));
    assert_eq!(position.commissions(), vec![Money::from("0.60 USDT")]);
    assert_eq!(cache.orders(None, None, None, None, None).len(), 3);

    for venue_order_id in [
        opening_venue_order_id,
        closing_venue_order_id,
        split_venue_order_id,
    ] {
        let order = cache
            .order(&ClientOrderId::from(venue_order_id.as_str()))
            .expect("reconciled terminal order");
        assert_eq!(order.status(), OrderStatus::Filled);
        assert_eq!(order.filled_qty(), Quantity::from("1.000"));
    }

    drop(position);
    drop(cache);

    let portfolio_events = portfolio_events.get_messages();

    let actual_portfolio_events: Vec<(&str, VenueOrderId)> = portfolio_events
        .iter()
        .map(|event| match event {
            OrderEventAny::Accepted(event) => ("accepted", event.venue_order_id),
            OrderEventAny::Filled(event) => ("filled", event.venue_order_id),
            event => panic!("Unexpected portfolio event: {event:?}"),
        })
        .collect();

    assert_eq!(
        actual_portfolio_events,
        vec![
            ("accepted", opening_venue_order_id),
            ("filled", opening_venue_order_id),
            ("filled", opening_venue_order_id),
            ("accepted", closing_venue_order_id),
            ("filled", closing_venue_order_id),
            ("accepted", split_venue_order_id),
            ("filled", split_venue_order_id),
        ]
    );
}

#[tokio::test]
async fn test_bounded_hedge_fill_applies_economics_once() {
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let strategy_id = StrategyId::from("EXTERNAL");
    let venue_order_id = VenueOrderId::from("V-BOUNDED-HEDGE");
    let venue_position_id = PositionId::from("P-BOUNDED-HEDGE");
    let trade_id = TradeId::from("T-BOUNDED-HEDGE");
    let cutoff = UnixNanos::from(1_500_000_000_000);
    let fill_ts = UnixNanos::from(1_500_001_000_000);

    ctx.add_instrument(instrument);

    let (mut order_report, mut fill_report) = create_bounded_fill_lifecycle(
        instrument_id,
        venue_order_id,
        trade_id,
        OrderSide::Buy,
        "1.000",
        "3000.00",
        false,
        fill_ts,
    );
    order_report = order_report.with_venue_position_id(venue_position_id);
    fill_report.venue_position_id = Some(venue_position_id);

    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("1.000"),
        fill_ts,
        fill_ts,
        None,
        Some(venue_position_id),
        Some(dec!(3000.00)),
    );

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        fill_ts,
        Some(UUID4::new()),
    );

    mass_status.set_report_window(Some(cutoff), true);
    mass_status.add_order_reports(vec![order_report]);
    mass_status.add_fill_reports(vec![fill_report]);
    mass_status.add_position_reports(vec![position_report]);

    let (portfolio_handler, portfolio_events) =
        get_typed_into_message_saving_handler::<OrderEventAny>(None);
    let portfolio_endpoint = MessagingSwitchboard::portfolio_update_order();
    msgbus::register_order_event_endpoint(portfolio_endpoint, portfolio_handler);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    msgbus::deregister_any(portfolio_endpoint);

    assert_eq!(result.events.len(), 2);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));

    let OrderEventAny::Filled(fill) = &result.events[1] else {
        panic!("Expected Filled event, was {:?}", result.events[1]);
    };

    assert_eq!(fill.strategy_id, strategy_id);
    assert_eq!(fill.venue_order_id, venue_order_id);
    assert_eq!(fill.position_id, Some(venue_position_id));
    assert_eq!(fill.trade_id, trade_id);
    assert_eq!(fill.last_qty, Quantity::from("1.000"));
    assert_eq!(result.external_orders.len(), 1);

    let cache = ctx.cache.borrow();
    let order = cache
        .order(&ClientOrderId::from(venue_order_id.as_str()))
        .expect("bounded hedge order");
    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.filled_qty(), Quantity::from("1.000"));
    let position = cache
        .position(&venue_position_id)
        .expect("bounded hedge position");
    assert_eq!(cache.oms_type(&venue_position_id), Some(OmsType::Hedging));
    assert!(position.is_open());
    assert!(position.is_long());
    assert_eq!(position.quantity, Quantity::from("1.000"));
    assert_eq!(position.trade_ids.len(), 1);
    assert!(position.trade_ids.contains(&trade_id));
    assert_eq!(position.commissions(), vec![Money::from("0.20 USDT")]);
    drop(position);
    drop(order);
    drop(cache);

    let portfolio_events = portfolio_events.get_messages();
    assert_eq!(portfolio_events.len(), 2);
    assert!(matches!(portfolio_events[0], OrderEventAny::Accepted(_)));
    assert!(matches!(portfolio_events[1], OrderEventAny::Filled(_)));
}

#[rstest]
#[case::sufficient_long_external(
    "STRATEGY-CLOSE",
    "1.000",
    false,
    OrderSide::Buy,
    OrderSide::Sell,
    true
)]
#[case::sufficient_long_cached(
    "STRATEGY-CLOSE",
    "1.000",
    true,
    OrderSide::Buy,
    OrderSide::Sell,
    true
)]
#[case::undersized_long_cached(
    "STRATEGY-CLOSE",
    "0.500",
    true,
    OrderSide::Buy,
    OrderSide::Sell,
    false
)]
#[case::unrelated_long_external(
    "STRATEGY-OTHER",
    "1.000",
    false,
    OrderSide::Buy,
    OrderSide::Sell,
    false
)]
#[case::sufficient_short_cached(
    "STRATEGY-CLOSE",
    "1.000",
    true,
    OrderSide::Sell,
    OrderSide::Buy,
    true
)]
#[case::undersized_short_cached(
    "STRATEGY-CLOSE",
    "0.500",
    true,
    OrderSide::Sell,
    OrderSide::Buy,
    false
)]
#[tokio::test]
async fn test_bounded_reduce_only_history_reconciles_flat_or_remains_unresolved(
    #[case] position_strategy: &str,
    #[case] position_qty: &str,
    #[case] cached_order: bool,
    #[case] opening_side: OrderSide,
    #[case] closing_side: OrderSide,
    #[case] closes_predecessor: bool,
) {
    let config = ExecutionManagerConfig {
        generate_missing_orders: false,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let close_strategy_id = StrategyId::from("STRATEGY-CLOSE");
    let position_strategy_id = StrategyId::from(position_strategy);
    let opening_order_id = ClientOrderId::from("O-CACHED-OPEN");
    let opening_venue_order_id = VenueOrderId::from("V-CACHED-OPEN");
    let opening_trade_id = TradeId::from("T-CACHED-OPEN");
    let closing_order_id = ClientOrderId::from("O-BOUNDED-CLOSE");
    let closing_venue_order_id = VenueOrderId::from("V-BOUNDED-CLOSE");
    let closing_trade_id = TradeId::from("T-BOUNDED-CLOSE");
    let position_id = PositionId::new(format!("{instrument_id}-{position_strategy_id}"));
    let close_position_id = PositionId::new(format!("{instrument_id}-{close_strategy_id}"));
    let cutoff = UnixNanos::from(2_000_000_000_000);
    let closing_ts = UnixNanos::from(2_000_001_000_000);

    ctx.add_instrument(instrument.clone());
    ctx.manager
        .claim_external_orders(instrument_id, close_strategy_id)
        .unwrap();
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(close_strategy_id, OmsType::Netting);
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(position_strategy_id, OmsType::Netting);

    let mut opening_order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(opening_order_id)
        .strategy_id(position_strategy_id)
        .instrument_id(instrument_id)
        .side(opening_side)
        .quantity(Quantity::from(position_qty))
        .price(Price::from("3000.00"))
        .build();
    apply_submitted_and_accepted(&mut opening_order, opening_venue_order_id);
    let opening_fill = TestOrderEventStubs::filled(
        &opening_order,
        &instrument,
        Some(opening_trade_id),
        Some(position_id),
        Some(Price::from("3000.00")),
        Some(Quantity::from(position_qty)),
        Some(LiquiditySide::Maker),
        Some(Money::from("0.10 USDT")),
        Some(UnixNanos::from(1_000_000_000_000)),
        Some(test_account_id()),
    );
    let position = Position::new(&instrument, opening_fill.into());
    ctx.cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();

    if cached_order {
        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .client_order_id(closing_order_id)
            .strategy_id(close_strategy_id)
            .instrument_id(instrument_id)
            .side(closing_side)
            .quantity(Quantity::from("1.000"))
            .price(Price::from("3100.00"))
            .reduce_only(true)
            .build();
        apply_submitted_and_accepted(&mut order, closing_venue_order_id);
        ctx.add_order(order);
    }

    let (mut closing_order, closing_fill) = create_bounded_fill_lifecycle(
        instrument_id,
        closing_venue_order_id,
        closing_trade_id,
        closing_side,
        "1.000",
        "3100.00",
        true,
        closing_ts,
    );

    if cached_order {
        closing_order.client_order_id = Some(closing_order_id);
    }

    let flat_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Flat,
        Quantity::from("0"),
        closing_ts,
        closing_ts,
        None,
        None,
        None,
    );

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        closing_ts,
        Some(UUID4::new()),
    );

    mass_status.set_report_window(Some(cutoff), true);
    mass_status.add_order_reports(vec![closing_order]);
    mass_status.add_fill_reports(vec![closing_fill]);
    mass_status.add_position_reports(vec![flat_report]);

    let (portfolio_handler, portfolio_events) =
        get_typed_into_message_saving_handler::<OrderEventAny>(None);
    let portfolio_endpoint = MessagingSwitchboard::portfolio_update_order();
    msgbus::register_order_event_endpoint(portfolio_endpoint, portfolio_handler);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    msgbus::deregister_any(portfolio_endpoint);

    assert_eq!(result.events.len(), if cached_order { 1 } else { 2 });

    if !cached_order {
        assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));
    }

    let OrderEventAny::Filled(fill) = result.events.last().unwrap() else {
        panic!(
            "Expected final Filled event, was {:?}",
            result.events.last()
        );
    };

    assert_eq!(fill.strategy_id, close_strategy_id);
    assert_eq!(fill.venue_order_id, closing_venue_order_id);
    assert_eq!(fill.trade_id, closing_trade_id);
    assert_eq!(fill.last_qty, Quantity::from("1.000"));
    assert_eq!(result.external_orders.len(), usize::from(!cached_order));

    let cache = ctx.cache.borrow();

    let reconciled_order_id = if cached_order {
        closing_order_id
    } else {
        ClientOrderId::from(closing_venue_order_id.as_str())
    };

    let order = cache
        .order(&reconciled_order_id)
        .expect("reconciled terminal order");
    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.filled_qty(), Quantity::from("1.000"));
    drop(order);
    let position = cache.position(&position_id).expect("cached predecessor");

    if closes_predecessor {
        assert!(position.is_closed());
        assert_eq!(position.quantity, Quantity::zero(3));
        assert_eq!(position.trade_ids.len(), 2);
        assert!(position.trade_ids.contains(&opening_trade_id));
        assert!(position.trade_ids.contains(&closing_trade_id));
        assert_eq!(position.commissions(), vec![Money::from("0.30 USDT")]);
    } else if position_strategy_id == close_strategy_id {
        assert!(position.is_open());
        assert_eq!(position.is_long(), closing_side == OrderSide::Buy);
        assert_eq!(position.quantity, Quantity::from("0.500"));
        assert_eq!(position.avg_px_open, dec!(3100.0));
        assert_eq!(position.trade_ids.len(), 1);
        assert!(position.trade_ids.contains(&closing_trade_id));
        assert_eq!(position.commissions(), vec![Money::from("0.10 USDT")]);
    } else {
        assert!(position.is_open());
        assert_eq!(position.is_long(), opening_side == OrderSide::Buy);
        assert_eq!(position.quantity, Quantity::from(position_qty));
        assert_eq!(position.trade_ids.len(), 1);
        assert!(position.trade_ids.contains(&opening_trade_id));
        assert_eq!(position.commissions(), vec![Money::from("0.10 USDT")]);
        assert!(cache.position(&close_position_id).is_none());
    }

    assert_eq!(
        result.unresolved_positions.len(),
        usize::from(!closes_predecessor)
    );

    drop(position);
    drop(cache);

    let portfolio_events = portfolio_events.get_messages();
    assert_eq!(portfolio_events.len(), usize::from(!cached_order) + 1);

    if !cached_order {
        assert!(matches!(portfolio_events[0], OrderEventAny::Accepted(_)));
    }

    assert!(matches!(
        portfolio_events.last(),
        Some(OrderEventAny::Filled(_))
    ));
}

#[rstest]
#[case::reported_fill(true)]
#[case::inferred_fill(false)]
#[tokio::test]
async fn test_incomplete_bounded_reports_project_fills_order_only(#[case] has_fill_report: bool) {
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let venue_order_id = VenueOrderId::from("V-INCOMPLETE-FILL");
    let trade_id = TradeId::from("T-INCOMPLETE-FILL");
    let cutoff = UnixNanos::from(3_000_000_000_000);
    let fill_ts = UnixNanos::from(3_000_001_000_000);

    ctx.add_instrument(instrument);
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(StrategyId::from("EXTERNAL"), OmsType::Netting);

    let (order_report, fill_report) = create_bounded_fill_lifecycle(
        instrument_id,
        venue_order_id,
        trade_id,
        OrderSide::Buy,
        "1.000",
        "3000.00",
        false,
        fill_ts,
    );

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        fill_ts,
        Some(UUID4::new()),
    );

    mass_status.set_report_window(Some(cutoff), false);
    mass_status.add_order_reports(vec![order_report]);
    if has_fill_report {
        mass_status.add_fill_reports(vec![fill_report]);
    }

    let (portfolio_handler, portfolio_events) =
        get_typed_into_message_saving_handler::<OrderEventAny>(None);
    let portfolio_endpoint = MessagingSwitchboard::portfolio_update_order();
    msgbus::register_order_event_endpoint(portfolio_endpoint, portfolio_handler);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    msgbus::deregister_any(portfolio_endpoint);

    assert_eq!(result.events.len(), 2);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));

    let OrderEventAny::Filled(fill) = &result.events[1] else {
        panic!("Expected Filled event, was {:?}", result.events[1]);
    };

    assert_eq!(fill.venue_order_id, venue_order_id);
    assert_eq!(fill.instrument_id, instrument_id);
    assert_eq!(fill.order_side, OrderSide::Buy);
    assert_eq!(fill.last_qty, Quantity::from("1.000"));
    assert_eq!(fill.last_px, Price::from("3000.00"));
    assert_eq!(fill.trade_id == trade_id, has_fill_report);
    assert!(fill.reconciliation);
    assert_eq!(result.external_orders.len(), 1);

    let cache = ctx.cache.borrow();
    let order = cache
        .order(&ClientOrderId::from(venue_order_id.as_str()))
        .expect("reconciled terminal order");
    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.filled_qty(), Quantity::from("1.000"));
    assert_eq!(cache.positions(None, None, None, None, None).len(), 0);
    drop(order);
    drop(cache);

    let portfolio_events = portfolio_events.get_messages();
    assert_eq!(portfolio_events.len(), 1);
    assert!(matches!(portfolio_events[0], OrderEventAny::Accepted(_)));
}

#[tokio::test]
async fn test_bounded_active_partial_order_preserves_order_and_reconciles_flat() {
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let venue_order_id = VenueOrderId::from("V-ACTIVE-PARTIAL");
    let cutoff = UnixNanos::from(4_000_000_000_000);
    let fill_ts = UnixNanos::from(4_000_001_000_000);

    ctx.add_instrument(instrument);
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(StrategyId::from("EXTERNAL"), OmsType::Netting);

    let order_report = OrderStatusReport::new(
        test_account_id(),
        instrument_id,
        None,
        venue_order_id,
        OrderSide::Buy.into(),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::PartiallyFilled,
        Quantity::from("1.000"),
        Quantity::from("0.400"),
        fill_ts,
        fill_ts,
        fill_ts,
        None,
    )
    .with_price(Price::from("3000.00"))
    .with_avg_px(dec!(3000.00));

    let flat_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Flat,
        Quantity::from("0"),
        fill_ts,
        fill_ts,
        None,
        None,
        None,
    );

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        fill_ts,
        Some(UUID4::new()),
    );

    mass_status.set_report_window(Some(cutoff), true);
    mass_status.add_order_reports(vec![order_report]);
    mass_status.add_position_reports(vec![flat_report]);

    let (portfolio_handler, portfolio_events) =
        get_typed_into_message_saving_handler::<OrderEventAny>(None);
    let portfolio_endpoint = MessagingSwitchboard::portfolio_update_order();
    msgbus::register_order_event_endpoint(portfolio_endpoint, portfolio_handler);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    msgbus::deregister_any(portfolio_endpoint);

    assert_eq!(result.events.len(), 4);
    assert!(result.unresolved_positions.is_empty());
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));

    let OrderEventAny::Filled(fill) = &result.events[1] else {
        panic!("Expected inferred Filled event, was {:?}", result.events[1]);
    };

    assert_eq!(fill.venue_order_id, venue_order_id);
    assert_eq!(fill.last_qty, Quantity::from("0.400"));
    assert_eq!(fill.last_px, Price::from("3000.00"));
    assert!(fill.reconciliation);
    assert_eq!(result.external_orders.len(), 1);

    let cache = ctx.cache.borrow();
    let order = cache
        .order(&ClientOrderId::from(venue_order_id.as_str()))
        .expect("active partially filled order");
    assert_eq!(order.status(), OrderStatus::PartiallyFilled);
    assert_eq!(order.quantity(), Quantity::from("1.000"));
    assert_eq!(order.filled_qty(), Quantity::from("0.400"));
    assert_eq!(cache.orders_open(None, None, None, None, None).len(), 1);
    assert_eq!(cache.positions_open(None, None, None, None, None).len(), 0);
    assert_eq!(cache.positions(None, None, None, None, None).len(), 1);
    drop(order);
    drop(cache);

    let portfolio_events = portfolio_events.get_messages();
    assert_eq!(portfolio_events.len(), 4);

    let closing_fill = match &portfolio_events[3] {
        OrderEventAny::Filled(fill) => fill,
        event => panic!("Expected closing fill, was {event:?}"),
    };

    assert_eq!(closing_fill.order_side, OrderSide::Sell);
    assert_eq!(closing_fill.last_qty, Quantity::from("0.400"));
    assert_eq!(closing_fill.last_px, Price::from("3000.00"));
    assert!(matches!(portfolio_events[0], OrderEventAny::Accepted(_)));
}

#[rstest]
#[case::coherent_open(OrderSide::Buy, false, true)]
#[case::isolated_reduce_only_close(OrderSide::Sell, true, false)]
#[tokio::test]
async fn test_bounded_nonflat_position_requires_coherent_historical_fill(
    #[case] order_side: OrderSide,
    #[case] reduce_only: bool,
    #[case] expected_applied: bool,
) {
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let strategy_id = StrategyId::from("EXTERNAL");
    let venue_order_id = VenueOrderId::from("V-BOUNDED-NONFLAT");
    let trade_id = TradeId::from("T-BOUNDED-NONFLAT");
    let cutoff = UnixNanos::from(5_000_000_000_000);
    let fill_ts = UnixNanos::from(5_000_001_000_000);

    ctx.add_instrument(instrument);
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(strategy_id, OmsType::Netting);

    let (order_report, fill_report) = create_bounded_fill_lifecycle(
        instrument_id,
        venue_order_id,
        trade_id,
        order_side,
        "1.000",
        "3000.00",
        reduce_only,
        fill_ts,
    );

    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("1.000"),
        fill_ts,
        fill_ts,
        None,
        None,
        Some(dec!(3000.00)),
    );

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        fill_ts,
        Some(UUID4::new()),
    );

    mass_status.set_report_window(Some(cutoff), true);
    mass_status.add_order_reports(vec![order_report]);
    mass_status.add_fill_reports(vec![fill_report]);
    mass_status.add_position_reports(vec![position_report]);

    let (portfolio_handler, portfolio_events) =
        get_typed_into_message_saving_handler::<OrderEventAny>(None);
    let portfolio_endpoint = MessagingSwitchboard::portfolio_update_order();
    msgbus::register_order_event_endpoint(portfolio_endpoint, portfolio_handler);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    msgbus::deregister_any(portfolio_endpoint);

    let historical_fill = result.events.iter().find_map(|event| match event {
        OrderEventAny::Filled(fill) if fill.venue_order_id == venue_order_id => Some(fill),
        _ => None,
    });

    let historical_fill = historical_fill.expect("historical fill event");
    assert_eq!(historical_fill.trade_id, trade_id);
    assert_eq!(historical_fill.order_side, order_side);
    assert_eq!(historical_fill.last_qty, Quantity::from("1.000"));
    assert_eq!(historical_fill.last_px, Price::from("3000.00"));
    assert!(result.unresolved_positions.is_empty());
    assert_eq!(result.events.len(), if expected_applied { 2 } else { 4 });
    assert_eq!(
        result.external_orders.len(),
        if expected_applied { 1 } else { 2 }
    );

    let cache = ctx.cache.borrow();
    let order = cache
        .order(&ClientOrderId::from(venue_order_id.as_str()))
        .expect("historical order");
    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.filled_qty(), Quantity::from("1.000"));
    assert_eq!(order.is_reduce_only(), reduce_only);
    let position_id = PositionId::new(format!("{instrument_id}-{strategy_id}"));
    let position = cache
        .position(&position_id)
        .expect("authoritative open position");
    assert!(position.is_open());
    assert!(position.is_long());
    assert_eq!(position.quantity, Quantity::from("1.000"));
    assert_eq!(position.avg_px_open, dec!(3000.0));
    assert!(position.trade_ids.contains(&trade_id));
    assert_eq!(
        position.trade_ids.len(),
        if expected_applied { 1 } else { 2 }
    );
    drop(position);
    drop(order);
    drop(cache);

    let portfolio_events = portfolio_events.get_messages();
    let historical_portfolio_fill = portfolio_events.iter().any(
        |event| matches!(event, OrderEventAny::Filled(fill) if fill.venue_order_id == venue_order_id),
    );
    assert!(historical_portfolio_fill);
    assert_eq!(portfolio_events.len(), if expected_applied { 2 } else { 4 });
}

#[rstest]
#[case::missing(0)]
#[case::duplicate_flat(2)]
#[tokio::test]
async fn test_bounded_history_distinguishes_missing_and_explicit_flat_reports(
    #[case] position_report_count: usize,
) {
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let strategy_id = StrategyId::from("EXTERNAL");
    let venue_order_id = VenueOrderId::from("V-BOUNDED-NO-POSITION");
    let trade_id = TradeId::from("T-BOUNDED-NO-POSITION");
    let cutoff = UnixNanos::from(6_000_000_000_000);
    let fill_ts = UnixNanos::from(6_000_001_000_000);

    ctx.add_instrument(instrument);
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(strategy_id, OmsType::Netting);

    let (order_report, fill_report) = create_bounded_fill_lifecycle(
        instrument_id,
        venue_order_id,
        trade_id,
        OrderSide::Buy,
        "1.000",
        "3000.00",
        false,
        fill_ts,
    );

    let position_reports = (0..position_report_count)
        .map(|_| {
            PositionStatusReport::new(
                test_account_id(),
                instrument_id,
                PositionSide::Flat,
                Quantity::zero(3),
                fill_ts,
                fill_ts,
                None,
                None,
                None,
            )
        })
        .collect();

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        fill_ts,
        Some(UUID4::new()),
    );

    mass_status.set_report_window(Some(cutoff), true);
    mass_status.add_order_reports(vec![order_report]);
    mass_status.add_fill_reports(vec![fill_report]);
    mass_status.add_position_reports(position_reports);

    let (portfolio_handler, portfolio_events) =
        get_typed_into_message_saving_handler::<OrderEventAny>(None);
    let portfolio_endpoint = MessagingSwitchboard::portfolio_update_order();
    msgbus::register_order_event_endpoint(portfolio_endpoint, portfolio_handler);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    msgbus::deregister_any(portfolio_endpoint);

    assert_eq!(
        result.events.len(),
        if position_report_count == 0 { 2 } else { 4 }
    );
    assert!(result.unresolved_positions.is_empty());
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));

    let OrderEventAny::Filled(fill) = &result.events[1] else {
        panic!("Expected Filled event, was {:?}", result.events[1]);
    };

    assert_eq!(fill.venue_order_id, venue_order_id);
    assert_eq!(fill.trade_id, trade_id);
    assert_eq!(fill.last_qty, Quantity::from("1.000"));
    assert_eq!(result.external_orders.len(), 1);

    let cache = ctx.cache.borrow();
    let order = cache
        .order(&ClientOrderId::from(venue_order_id.as_str()))
        .expect("historical order");
    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.filled_qty(), Quantity::from("1.000"));
    assert_eq!(cache.positions_open(None, None, None, None, None).len(), 0);
    assert_eq!(
        cache.positions(None, None, None, None, None).len(),
        usize::from(position_report_count != 0)
    );
    drop(order);
    drop(cache);

    let portfolio_events = portfolio_events.get_messages();
    assert_eq!(
        portfolio_events.len(),
        if position_report_count == 0 { 1 } else { 4 }
    );
    assert!(matches!(portfolio_events[0], OrderEventAny::Accepted(_)));
}

#[tokio::test]
async fn test_bounded_interleaved_multi_fill_orders_reconcile_explicit_flat() {
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let strategy_id = StrategyId::from("EXTERNAL");
    let opening_venue_order_id = VenueOrderId::from("V-INTERLEAVED-OPEN");
    let closing_venue_order_id = VenueOrderId::from("V-INTERLEAVED-CLOSE");
    let opening_trade_id = TradeId::from("T-INTERLEAVED-OPEN-1");
    let opening_trade_id_2 = TradeId::from("T-INTERLEAVED-OPEN-2");
    let closing_trade_id = TradeId::from("T-INTERLEAVED-CLOSE");
    let cutoff = UnixNanos::from(7_000_000_000_000);
    let opening_ts = UnixNanos::from(7_000_001_000_000);
    let closing_ts = UnixNanos::from(7_000_002_000_000);
    let opening_ts_2 = UnixNanos::from(7_000_003_000_000);

    ctx.add_instrument(instrument);
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(strategy_id, OmsType::Netting);

    let (mut opening_order, opening_fill) = create_bounded_fill_lifecycle(
        instrument_id,
        opening_venue_order_id,
        opening_trade_id,
        OrderSide::Buy,
        "0.500",
        "3000.00",
        false,
        opening_ts,
    );
    opening_order.quantity = Quantity::from("1.000");
    opening_order.filled_qty = Quantity::from("1.000");
    let (_, opening_fill_2) = create_bounded_fill_lifecycle(
        instrument_id,
        opening_venue_order_id,
        opening_trade_id_2,
        OrderSide::Buy,
        "0.500",
        "3000.00",
        false,
        opening_ts_2,
    );
    let (closing_order, closing_fill) = create_bounded_fill_lifecycle(
        instrument_id,
        closing_venue_order_id,
        closing_trade_id,
        OrderSide::Sell,
        "1.000",
        "3100.00",
        true,
        closing_ts,
    );

    let flat_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Flat,
        Quantity::zero(3),
        opening_ts_2,
        opening_ts_2,
        None,
        None,
        None,
    );

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        opening_ts_2,
        Some(UUID4::new()),
    );

    mass_status.set_report_window(Some(cutoff), true);
    mass_status.add_order_reports(vec![opening_order, closing_order]);
    mass_status.add_fill_reports(vec![opening_fill, opening_fill_2, closing_fill]);
    mass_status.add_position_reports(vec![flat_report]);

    let (portfolio_handler, portfolio_events) =
        get_typed_into_message_saving_handler::<OrderEventAny>(None);
    let portfolio_endpoint = MessagingSwitchboard::portfolio_update_order();
    msgbus::register_order_event_endpoint(portfolio_endpoint, portfolio_handler);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    msgbus::deregister_any(portfolio_endpoint);

    let actual_events: Vec<(&str, VenueOrderId, Option<TradeId>)> = result
        .events
        .iter()
        .map(|event| match event {
            OrderEventAny::Accepted(event) => ("accepted", event.venue_order_id, None),
            OrderEventAny::Filled(event) => ("filled", event.venue_order_id, Some(event.trade_id)),
            event => panic!("Unexpected reconciliation event: {event:?}"),
        })
        .collect();

    assert_eq!(
        actual_events,
        vec![
            ("accepted", opening_venue_order_id, None),
            ("filled", opening_venue_order_id, Some(opening_trade_id)),
            ("accepted", closing_venue_order_id, None),
            ("filled", closing_venue_order_id, Some(closing_trade_id)),
            ("filled", opening_venue_order_id, Some(opening_trade_id_2)),
        ]
    );
    assert_eq!(result.external_orders.len(), 2);

    let cache = ctx.cache.borrow();
    for venue_order_id in [opening_venue_order_id, closing_venue_order_id] {
        let order = cache
            .order(&ClientOrderId::from(venue_order_id.as_str()))
            .expect("historical order");
        assert_eq!(order.status(), OrderStatus::Filled);
        assert_eq!(order.filled_qty(), Quantity::from("1.000"));
    }

    assert!(result.unresolved_positions.is_empty());
    assert_eq!(cache.positions_open(None, None, None, None, None).len(), 0);
    assert_eq!(cache.positions(None, None, None, None, None).len(), 1);
    drop(cache);

    let portfolio_events = portfolio_events.get_messages();
    let position_id = PositionId::new(format!("{instrument_id}-{strategy_id}"));

    let expected_portfolio_events: Vec<_> = result
        .events
        .iter()
        .cloned()
        .map(|mut event| {
            if let OrderEventAny::Filled(fill) = &mut event {
                fill.position_id = Some(position_id);
            }

            event
        })
        .collect();

    assert_eq!(portfolio_events, expected_portfolio_events);
}

#[tokio::test]
async fn test_bounded_same_timestamp_orders_reconcile_explicit_flat() {
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let strategy_id = StrategyId::from("EXTERNAL");
    let opening_venue_order_id = VenueOrderId::from("V-SAME-TS-OPEN");
    let closing_venue_order_id = VenueOrderId::from("V-SAME-TS-CLOSE");
    let opening_trade_id = TradeId::from("T-SAME-TS-OPEN");
    let closing_trade_id = TradeId::from("T-SAME-TS-CLOSE");
    let cutoff = UnixNanos::from(8_000_000_000_000);
    let fill_ts = UnixNanos::from(8_000_001_000_000);

    ctx.add_instrument(instrument);
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(strategy_id, OmsType::Netting);

    let (opening_order, opening_fill) = create_bounded_fill_lifecycle(
        instrument_id,
        opening_venue_order_id,
        opening_trade_id,
        OrderSide::Buy,
        "1.000",
        "3000.00",
        false,
        fill_ts,
    );
    let (closing_order, closing_fill) = create_bounded_fill_lifecycle(
        instrument_id,
        closing_venue_order_id,
        closing_trade_id,
        OrderSide::Sell,
        "1.000",
        "3100.00",
        true,
        fill_ts,
    );

    let flat_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Flat,
        Quantity::zero(3),
        fill_ts,
        fill_ts,
        None,
        None,
        None,
    );

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        fill_ts,
        Some(UUID4::new()),
    );

    mass_status.set_report_window(Some(cutoff), true);
    mass_status.add_order_reports(vec![opening_order, closing_order]);
    mass_status.add_fill_reports(vec![opening_fill, closing_fill]);
    mass_status.add_position_reports(vec![flat_report]);

    let (portfolio_handler, portfolio_events) =
        get_typed_into_message_saving_handler::<OrderEventAny>(None);
    let portfolio_endpoint = MessagingSwitchboard::portfolio_update_order();
    msgbus::register_order_event_endpoint(portfolio_endpoint, portfolio_handler);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    msgbus::deregister_any(portfolio_endpoint);

    let actual_events: Vec<(&str, VenueOrderId, Option<TradeId>)> = result
        .events
        .iter()
        .map(|event| match event {
            OrderEventAny::Accepted(event) => ("accepted", event.venue_order_id, None),
            OrderEventAny::Filled(event) => ("filled", event.venue_order_id, Some(event.trade_id)),
            event => panic!("Unexpected reconciliation event: {event:?}"),
        })
        .collect();

    assert_eq!(
        actual_events,
        vec![
            ("accepted", opening_venue_order_id, None),
            ("filled", opening_venue_order_id, Some(opening_trade_id)),
            ("accepted", closing_venue_order_id, None),
            ("filled", closing_venue_order_id, Some(closing_trade_id)),
        ]
    );
    assert_eq!(result.external_orders.len(), 2);

    let cache = ctx.cache.borrow();
    for venue_order_id in [opening_venue_order_id, closing_venue_order_id] {
        let order = cache
            .order(&ClientOrderId::from(venue_order_id.as_str()))
            .expect("same-timestamp historical order");
        assert_eq!(order.status(), OrderStatus::Filled);
        assert_eq!(order.filled_qty(), Quantity::from("1.000"));
    }

    assert!(result.unresolved_positions.is_empty());
    assert_eq!(cache.positions_open(None, None, None, None, None).len(), 0);
    assert_eq!(cache.positions(None, None, None, None, None).len(), 1);
    drop(cache);

    let portfolio_events = portfolio_events.get_messages();
    let position_id = PositionId::new(format!("{instrument_id}-{strategy_id}"));

    let expected_portfolio_events: Vec<_> = result
        .events
        .iter()
        .cloned()
        .map(|mut event| {
            if let OrderEventAny::Filled(fill) = &mut event {
                fill.position_id = Some(position_id);
            }

            event
        })
        .collect();

    assert_eq!(portfolio_events, expected_portfolio_events);
}

/// An order whose fills span a position lifecycle keeps its pre-boundary fill as order-only.
///
/// A sell that closes a long and opens a short crosses zero inside the order. The closing fill
/// stays on the order, so its filled quantity settles at the real price rather than an inferred
/// one, and it does not apply to the position.
#[tokio::test]
async fn test_order_spanning_two_lifecycles_keeps_pre_boundary_fill_order_only() {
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let strategy_id = StrategyId::from("STRATEGY-001");
    let open_order_id = ClientOrderId::from("O-LIFECYCLE-OPEN");
    let open_venue_order_id = VenueOrderId::from("V-LIFECYCLE-OPEN");
    let open_trade_id = TradeId::from("T-LIFECYCLE-OPEN");
    let flip_order_id = ClientOrderId::from("O-LIFECYCLE-FLIP");
    let flip_venue_order_id = VenueOrderId::from("V-LIFECYCLE-FLIP");
    let close_trade_id = TradeId::from("T-LIFECYCLE-CLOSE");
    let short_trade_id = TradeId::from("T-LIFECYCLE-SHORT");
    let position_id = PositionId::new(format!("{instrument_id}-{strategy_id}"));

    ctx.add_instrument(instrument);
    ctx.manager
        .claim_external_orders(instrument_id, strategy_id)
        .unwrap();
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(strategy_id, OmsType::Netting);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // The long that the flip closes, opened and filled before the crossing.
    let open_report = create_order_report(
        Some(open_order_id),
        open_venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("1.000"),
        Quantity::from("1.000"),
    )
    .with_avg_px(dec!(3000.0));

    let open_fill = FillReport::new(
        test_account_id(),
        instrument_id,
        open_venue_order_id,
        open_trade_id,
        OrderSide::Buy,
        Quantity::from("1.000"),
        Price::from("3000.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Maker,
        Some(open_order_id),
        None,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );

    // The sell spanning the crossing: one fill closes the long, the next opens the short. Its
    // order price sits below both fill prices, so an inferred remainder would be visible.
    let flip_report = create_order_report_for_side(
        Some(flip_order_id),
        flip_venue_order_id,
        instrument_id,
        OrderSide::Sell,
        OrderStatus::Filled,
        Quantity::from("2.000"),
        Quantity::from("2.000"),
    )
    .with_price(Price::from("3000.00"))
    .with_avg_px(dec!(3150.0));

    let close_fill = FillReport::new(
        test_account_id(),
        instrument_id,
        flip_venue_order_id,
        close_trade_id,
        OrderSide::Sell,
        Quantity::from("1.000"),
        Price::from("3100.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Taker,
        Some(flip_order_id),
        None,
        UnixNanos::from(2_000_000),
        UnixNanos::from(2_000_000),
        None,
    );

    let short_fill = FillReport::new(
        test_account_id(),
        instrument_id,
        flip_venue_order_id,
        short_trade_id,
        OrderSide::Sell,
        Quantity::from("1.000"),
        Price::from("3200.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Taker,
        Some(flip_order_id),
        None,
        UnixNanos::from(3_000_000),
        UnixNanos::from(3_000_000),
        None,
    );

    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Short,
        Quantity::from("1.000"),
        UnixNanos::from(3_000_000),
        UnixNanos::from(3_000_000),
        None,
        None,
        Some(dec!(3200.00)),
    );

    mass_status.add_order_reports(vec![open_report, flip_report]);
    mass_status.add_fill_reports(vec![open_fill, close_fill, short_fill]);
    mass_status.add_position_reports(vec![position_report]);

    ctx.manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let cache = ctx.cache.borrow();
    let flip_order = cache.order(&flip_order_id).unwrap();
    assert_eq!(flip_order.status(), OrderStatus::Filled);
    assert_eq!(flip_order.filled_qty(), Quantity::from("2.000"));
    assert!(
        flip_order.trade_ids().contains(&&close_trade_id),
        "the closing fill must settle the order rather than be inferred: {:?}",
        flip_order.trade_ids()
    );
    assert!(flip_order.trade_ids().contains(&&short_trade_id));
    assert_eq!(
        flip_order.avg_px(),
        Some(dec!(3150.0)),
        "both real fills price the order; nothing is inferred at the 3000 order price"
    );

    let position = cache.position(&position_id).unwrap();
    assert_eq!(position.side, PositionSide::Short);
    assert_eq!(position.quantity, Quantity::from("1.000"));
    assert!(
        !position.trade_ids.contains(&close_trade_id),
        "the pre-boundary fill must not apply to the current lifecycle"
    );
    assert!(position.trade_ids.contains(&short_trade_id));
}

/// A partially filled cached order recovers its remaining executions at their real prices.
///
/// With 0.600 of 1.000 already recorded and the final 0.400 executing offline, the fills page
/// carries only that 0.400. Reconciliation matches it to the cached order rather than inferring
/// it, so the adapter must not withhold the report for not covering the full quantity on its own.
#[tokio::test]
async fn test_partially_filled_cached_order_recovers_remaining_fills_at_real_prices() {
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let strategy_id = StrategyId::from("STRATEGY-001");
    let client_order_id = ClientOrderId::from("O-PARTIAL-CACHED");
    let venue_order_id = VenueOrderId::from("V-PARTIAL-CACHED");
    let cached_trade_id = TradeId::from("T-PARTIAL-CACHED");
    let remaining_trade_id = TradeId::from("T-PARTIAL-REMAINING");
    let position_id = PositionId::new(format!("{instrument_id}-{strategy_id}"));

    ctx.add_instrument(instrument.clone());
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(strategy_id, OmsType::Netting);

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .strategy_id(strategy_id)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .price(Price::from("3000.00"))
        .build();
    apply_submitted_and_accepted(&mut order, venue_order_id);
    let cached_fill = TestOrderEventStubs::filled(
        &order,
        &instrument,
        Some(cached_trade_id),
        Some(position_id),
        Some(Price::from("3000.00")),
        Some(Quantity::from("0.600")),
        Some(LiquiditySide::Maker),
        Some(Money::from("0.30 USDT")),
        Some(UnixNanos::from(1_000_000)),
        Some(test_account_id()),
    );
    order.apply(cached_fill).unwrap();

    {
        let mut cache = ctx.cache.borrow_mut();
        cache
            .add_order(order, None, Some(test_client_id()), false)
            .unwrap();
        cache
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

    let order_report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("1.000"),
        Quantity::from("1.000"),
    )
    .with_avg_px(dec!(3050.0));

    let remaining_fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        remaining_trade_id,
        OrderSide::Buy,
        Quantity::from("0.400"),
        Price::from("3125.00"),
        Money::from("0.20 USDT"),
        LiquiditySide::Taker,
        Some(client_order_id),
        None,
        UnixNanos::from(2_000_000),
        UnixNanos::from(2_000_000),
        None,
    );

    mass_status.add_order_reports(vec![order_report]);
    mass_status.add_fill_reports(vec![remaining_fill]);

    ctx.manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let cache = ctx.cache.borrow();
    let order = cache.order(&client_order_id).unwrap();
    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.filled_qty(), Quantity::from("1.000"));
    assert!(order.trade_ids().contains(&&cached_trade_id));
    assert!(
        order.trade_ids().contains(&&remaining_trade_id),
        "the remaining execution must be applied from its fill report: {:?}",
        order.trade_ids()
    );
    assert_eq!(
        order.trade_ids().len(),
        2,
        "no inferred fill alongside the real ones"
    );
    assert_eq!(
        order.avg_px(),
        Some(dec!(3050.0)),
        "0.600 at 3000 and 0.400 at 3125 price the order; nothing is inferred"
    );
}

#[rstest]
#[case::cached_unbounded(ClosingOrder::Cached, false)]
#[case::cached_bounded(ClosingOrder::Cached, true)]
#[case::claimed_unbounded(ClosingOrder::Claimed, false)]
#[case::claimed_bounded(ClosingOrder::Claimed, true)]
fn test_closing_order_fills_close_restored_position_without_coverage(
    #[case] closing: ClosingOrder,
    #[case] bounded: bool,
) {
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    cache_closing_scenario_open_long(&ctx, &test_instrument());
    let (closing_order_id, reported_order_id) = prepare_closing_scenario_order(&mut ctx, closing);

    let mass_status = closing_scenario_mass_status(bounded, reported_order_id);
    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());
    assert_eq!(
        result.external_orders.len(),
        usize::from(matches!(closing, ClosingOrder::Claimed))
    );
    assert_eq!(count_filled_events(&result.events), 2);
    assert_closing_scenario_recorded_once(&ctx.cache.borrow(), closing_order_id);
    assert_closing_scenario_position_events(&position_events);
}

#[rstest]
#[case::cached_unbounded(ClosingOrder::Cached, false)]
#[case::cached_bounded(ClosingOrder::Cached, true)]
#[case::claimed_unbounded(ClosingOrder::Claimed, false)]
#[case::claimed_bounded(ClosingOrder::Claimed, true)]
#[tokio::test]
async fn test_closing_order_fills_recorded_once_across_restart_without_coverage(
    #[case] closing: ClosingOrder,
    #[case] bounded: bool,
) {
    let (closing_order_id, reported_order_id, restored) = {
        let mut ctx =
            closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
        cache_closing_scenario_open_long(&ctx, &test_instrument());
        let (closing_order_id, reported_order_id) =
            prepare_closing_scenario_order(&mut ctx, closing);

        let mass_status = closing_scenario_mass_status(bounded, reported_order_id);
        let (result, _) = reconcile_closing_scenario(&mut ctx, &mass_status);
        assert_eq!(count_filled_events(&result.events), 2);
        assert_closing_scenario_recorded_once(&ctx.cache.borrow(), closing_order_id);

        (
            closing_order_id,
            reported_order_id,
            restore_closing_cache(&ctx.cache.borrow()),
        )
    };

    let mut ctx = closing_scenario_context(restored, ExecutionManagerConfig::default(), false);
    claim_closing_scenario_instrument(&mut ctx, closing);

    let mass_status = closing_scenario_mass_status(bounded, reported_order_id);
    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());
    assert!(result.external_orders.is_empty());
    assert_eq!(count_filled_events(&result.events), 0);
    assert!(
        position_events.is_empty(),
        "restart must not re-apply fills, found {position_events:?}"
    );
    assert_closing_scenario_recorded_once(&ctx.cache.borrow(), closing_order_id);
}

fn assert_unclaimed_closing_scenario_state(cache: &Cache, bounded: bool) {
    let closing_order_id = ClientOrderId::from(closing_scenario_venue_order_id().as_str());
    if bounded {
        assert_closing_scenario_order_only(cache, closing_order_id);
        return;
    }

    assert_closing_scenario_order_filled(cache, closing_order_id);
    assert_closing_scenario_position_unchanged(cache);

    let external_position_id = PositionId::new(format!("{}-EXTERNAL", test_instrument_id()));
    assert_eq!(
        closing_scenario_instrument_position_ids(cache),
        vec![external_position_id, closing_scenario_position_id()]
    );
    let external_position = cache
        .position(&external_position_id)
        .expect("external position cached");
    assert!(external_position.is_open());
    assert_eq!(external_position.side, PositionSide::Short);
    assert_eq!(external_position.quantity, Quantity::from("1.000"));
    assert_eq!(
        external_position.commissions(),
        vec![Money::from("0.62 USDT")]
    );
    assert_eq!(
        external_position.trade_ids,
        [TradeId::from("T-CLOSE-1"), TradeId::from("T-CLOSE-2")]
            .into_iter()
            .collect::<AHashSet<_>>()
    );
}

#[rstest]
#[case::unbounded(false)]
#[case::bounded(true)]
#[tokio::test]
async fn test_unclaimed_external_closing_order_fills_leave_restored_position_open(
    #[case] bounded: bool,
) {
    let restored = {
        let mut ctx =
            closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
        cache_closing_scenario_open_long(&ctx, &test_instrument());
        let (_, reported_order_id) =
            prepare_closing_scenario_order(&mut ctx, ClosingOrder::Unclaimed);

        let mass_status = closing_scenario_mass_status(bounded, reported_order_id);
        let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

        assert!(result.unresolved_positions.is_empty());
        assert_eq!(result.external_orders.len(), 1);
        assert_eq!(count_filled_events(&result.events), 2);
        assert_unclaimed_closing_scenario_state(&ctx.cache.borrow(), bounded);
        if bounded {
            assert!(position_events.is_empty(), "found {position_events:?}");
        } else {
            assert_eq!(position_events.len(), 2, "found {position_events:?}");
            assert!(matches!(
                position_events[0],
                PositionEvent::PositionOpened(_)
            ));
            assert!(matches!(
                position_events[1],
                PositionEvent::PositionChanged(_)
            ));
        }

        restore_closing_cache(&ctx.cache.borrow())
    };

    let mut ctx = closing_scenario_context(restored, ExecutionManagerConfig::default(), false);

    let mass_status = closing_scenario_mass_status(bounded, None);
    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());
    assert!(result.external_orders.is_empty());
    assert_eq!(count_filled_events(&result.events), 0);
    assert!(
        position_events.is_empty(),
        "restart must not re-apply fills, found {position_events:?}"
    );
    assert_unclaimed_closing_scenario_state(&ctx.cache.borrow(), bounded);
}

#[rstest]
#[case::cached(ClosingOrder::Cached)]
#[case::claimed(ClosingOrder::Claimed)]
#[tokio::test]
async fn test_bounded_closing_fills_stay_order_only_when_engine_oms_is_hedging(
    #[case] closing: ClosingOrder,
) {
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    cache_closing_scenario_open_long(&ctx, &test_instrument());
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(closing_scenario_strategy_id(), OmsType::Hedging);
    let (closing_order_id, reported_order_id) = prepare_closing_scenario_order(&mut ctx, closing);

    let mass_status = closing_scenario_mass_status(true, reported_order_id);
    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());
    assert_eq!(count_filled_events(&result.events), 2);
    assert!(position_events.is_empty(), "found {position_events:?}");
    assert_closing_scenario_order_only(&ctx.cache.borrow(), closing_order_id);
}

#[rstest]
#[case::indexed(true)]
#[case::explicit(false)]
fn test_bounded_closing_fills_close_known_hedging_position(#[case] indexed: bool) {
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    cache_closing_scenario_open_position(
        &ctx,
        &test_instrument(),
        test_account_id(),
        OmsType::Hedging,
        OrderSide::Buy,
    );
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(closing_scenario_strategy_id(), OmsType::Hedging);
    let (order_id, reported_id) = prepare_closing_scenario_order(&mut ctx, ClosingOrder::Cached);

    let mut mass = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::from(CLOSING_SCENARIO_FILL_TS_2 + 1_000_000),
        None,
    );

    mass.set_report_window(Some(UnixNanos::from(CLOSING_SCENARIO_WINDOW_START)), true);
    mass.add_order_reports(vec![closing_scenario_closing_order_report(reported_id)]);
    let mut fills = closing_scenario_closing_fill_reports(reported_id);

    if indexed {
        ctx.cache
            .borrow_mut()
            .add_position_id(
                &closing_scenario_position_id(),
                &test_venue(),
                &order_id,
                &closing_scenario_strategy_id(),
            )
            .unwrap();
    } else {
        for fill in &mut fills {
            fill.venue_position_id = Some(closing_scenario_position_id());
        }
    }

    mass.add_fill_reports(fills);

    let (_, events) = reconcile_closing_scenario(&mut ctx, &mass);

    assert_closing_scenario_recorded_once(&ctx.cache.borrow(), order_id);
    assert_closing_scenario_position_events(&events);
}

#[rstest]
#[case::covered_cached(ClosingOrder::Cached, true, true, false)]
#[case::covered_claimed(ClosingOrder::Claimed, true, true, false)]
#[case::incomplete_cached(ClosingOrder::Cached, false, false, false)]
#[case::incomplete_claimed(ClosingOrder::Claimed, false, false, false)]
#[case::filtered_cached(ClosingOrder::Cached, false, true, true)]
#[case::filtered_claimed(ClosingOrder::Claimed, false, true, true)]
fn test_bounded_closing_order_fills_stay_order_only(
    #[case] closing: ClosingOrder,
    #[case] covered: bool,
    #[case] reports_complete: bool,
    #[case] filter_position_reports: bool,
) {
    let config = ExecutionManagerConfig {
        filter_position_reports,
        ..ExecutionManagerConfig::default()
    };

    let mut ctx = closing_scenario_context(Cache::default(), config, covered);
    cache_closing_scenario_open_long(&ctx, &test_instrument());
    let (closing_order_id, reported_order_id) = prepare_closing_scenario_order(&mut ctx, closing);

    let mut mass_status = closing_scenario_mass_status(true, reported_order_id);
    mass_status.set_report_window(
        Some(UnixNanos::from(CLOSING_SCENARIO_WINDOW_START)),
        reports_complete,
    );
    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());
    assert_eq!(count_filled_events(&result.events), 2);
    assert!(position_events.is_empty(), "found {position_events:?}");
    assert_closing_scenario_order_only(&ctx.cache.borrow(), closing_order_id);
}

#[rstest]
#[case::cached(ClosingOrder::Cached)]
#[case::claimed(ClosingOrder::Claimed)]
fn test_bounded_closing_order_fills_without_retained_position_stay_order_only(
    #[case] closing: ClosingOrder,
) {
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    let (closing_order_id, reported_order_id) = prepare_closing_scenario_order(&mut ctx, closing);

    let mass_status = closing_scenario_mass_status(true, reported_order_id);
    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());
    assert_eq!(count_filled_events(&result.events), 2);
    assert!(position_events.is_empty(), "found {position_events:?}");
    assert_closing_scenario_order_filled(&ctx.cache.borrow(), closing_order_id);
    assert!(closing_scenario_instrument_position_ids(&ctx.cache.borrow()).is_empty());
}

#[rstest]
#[case::crossing(OrderSide::Sell)]
#[case::extending(OrderSide::Buy)]
fn test_bounded_closing_fills_do_not_reopen_or_flip_position(#[case] extra_side: OrderSide) {
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    cache_closing_scenario_open_long(&ctx, &test_instrument());
    let (closing_order_id, reported_order_id) =
        prepare_closing_scenario_order(&mut ctx, ClosingOrder::Cached);

    let extra_order_id = ClientOrderId::from("O-EXTRA");
    let extra_venue_order_id = VenueOrderId::from("V-EXTRA");
    let mut extra_order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(extra_order_id)
        .strategy_id(closing_scenario_strategy_id())
        .instrument_id(test_instrument_id())
        .side(extra_side)
        .quantity(Quantity::from("0.500"))
        .price(Price::from("3130.00"))
        .build();
    apply_submitted_and_accepted(&mut extra_order, extra_venue_order_id);
    ctx.add_order(extra_order);

    let (mut extra_report, mut extra_fill) = create_bounded_fill_lifecycle(
        test_instrument_id(),
        extra_venue_order_id,
        TradeId::from("T-EXTRA"),
        extra_side,
        "0.500",
        "3130.00",
        false,
        UnixNanos::from(CLOSING_SCENARIO_FILL_TS_2 + 500_000),
    );
    extra_report.client_order_id = Some(extra_order_id);
    extra_fill.client_order_id = Some(extra_order_id);

    let mut mass_status = closing_scenario_mass_status(true, reported_order_id);
    mass_status.add_order_reports(vec![extra_report]);
    mass_status.add_fill_reports(vec![extra_fill]);
    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());
    assert_eq!(count_filled_events(&result.events), 3);
    assert_closing_scenario_position_events(&position_events);
    assert_closing_scenario_recorded_once(&ctx.cache.borrow(), closing_order_id);
    let extra_order = ctx.get_order(&extra_order_id).expect("extra order cached");
    assert_eq!(extra_order.status(), OrderStatus::Filled);
    assert_eq!(extra_order.filled_qty(), Quantity::from("0.500"));
}

#[rstest]
#[case::cached_addition(true)]
#[case::unclaimed_addition(false)]
#[tokio::test]
async fn test_bounded_addition_before_close_warns_for_order_only_addition(
    #[case] cached_addition: bool,
) {
    let _log_guard = MANAGER_LOG_TEST_LOCK.lock().await;
    install_manager_log_capture();

    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    cache_closing_scenario_open_long(&ctx, &test_instrument());
    let (closing_order_id, reported_order_id) =
        prepare_closing_scenario_order(&mut ctx, ClosingOrder::Cached);

    let add_venue_order_id = VenueOrderId::from("V-WARN-ADD");

    let add_order_id = if cached_addition {
        ClientOrderId::from("O-WARN-ADD")
    } else {
        ClientOrderId::from(add_venue_order_id.as_str())
    };

    let add_report_order_id = cached_addition.then_some(add_order_id);
    if cached_addition {
        let mut add_order = OrderTestBuilder::new(OrderType::Limit)
            .client_order_id(add_order_id)
            .strategy_id(closing_scenario_strategy_id())
            .instrument_id(test_instrument_id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.000"))
            .price(Price::from("2900.00"))
            .build();
        apply_submitted_and_accepted(&mut add_order, add_venue_order_id);
        ctx.add_order(add_order);
    }

    let (mut add_report, mut add_fill) = create_bounded_fill_lifecycle(
        test_instrument_id(),
        add_venue_order_id,
        TradeId::from("T-WARN-ADD"),
        OrderSide::Buy,
        "1.000",
        "2900.00",
        false,
        UnixNanos::from(CLOSING_SCENARIO_FILL_TS_1 - 500_000),
    );
    add_report.client_order_id = add_report_order_id;
    add_fill.client_order_id = add_report_order_id;

    let mut mass_status = closing_scenario_mass_status(true, reported_order_id);
    mass_status.add_order_reports(vec![add_report]);
    mass_status.add_fill_reports(vec![add_fill.clone()]);
    let (result, _) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());
    assert_eq!(count_filled_events(&result.events), 3);
    assert_closing_scenario_recorded_once(&ctx.cache.borrow(), closing_order_id);
    let add_order = ctx.get_order(&add_order_id).expect("addition order cached");
    assert_eq!(add_order.status(), OrderStatus::Filled);

    let target = if cached_addition {
        format!("not to position {}", closing_scenario_position_id())
    } else {
        "with no resolvable position".to_string()
    };

    assert_eq!(
        withheld_fill_warnings("T-WARN-ADD"),
        vec![format!(
            "Applied 1 bounded fill(s) for {} on {} to order state only, {target} \
             (T-WARN-ADD ({add_order_id})): exposure and PnL may need manual reconciliation",
            test_instrument_id(),
            test_account_id(),
        )]
    );

    // The restart reports the addition's fill without its order report, so the fill is
    // rebuilt against the cached order that already holds it
    MANAGER_LOG_CAPTURE.messages.lock().clear();
    let mut ctx = closing_scenario_context(
        restore_closing_cache(&ctx.cache.borrow()),
        ExecutionManagerConfig::default(),
        false,
    );
    let mut mass_status = closing_scenario_mass_status(true, reported_order_id);
    mass_status.add_fill_reports(vec![add_fill]);
    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert_eq!(count_filled_events(&result.events), 1);
    assert!(position_events.is_empty(), "found {position_events:?}");
    assert!(
        withheld_fill_warnings("T-WARN-ADD").is_empty(),
        "found {:?}",
        MANAGER_LOG_CAPTURE.messages.lock()
    );
}

#[rstest]
#[case::recovered(ClosingOrder::Cached, false, true, false, true, false)]
#[case::covered(ClosingOrder::Cached, true, true, false, true, false)]
#[case::incomplete_cached(ClosingOrder::Cached, false, false, false, true, true)]
#[case::incomplete_claimed(ClosingOrder::Claimed, false, false, false, true, true)]
#[case::filtered(ClosingOrder::Cached, false, true, true, true, true)]
#[case::unbounded(ClosingOrder::Cached, false, true, false, false, false)]
#[tokio::test]
async fn test_bounded_order_only_fills_warn_without_bulk_position_coverage(
    #[case] closing: ClosingOrder,
    #[case] covered: bool,
    #[case] reports_complete: bool,
    #[case] filter_position_reports: bool,
    #[case] bounded: bool,
    #[case] warns: bool,
) {
    let _log_guard = MANAGER_LOG_TEST_LOCK.lock().await;
    install_manager_log_capture();

    let tag = "WARN-GATE";

    let config = ExecutionManagerConfig {
        filter_position_reports,
        ..ExecutionManagerConfig::default()
    };

    let mut ctx = closing_scenario_context(Cache::default(), config, covered);
    cache_closing_scenario_open_long(&ctx, &test_instrument());
    let (closing_order_id, reported_order_id) =
        prepare_closing_scenario_order_for(&mut ctx, closing, tag);
    let (order_report, fill_reports) = withheld_scenario_reports(tag, reported_order_id);
    let mass_status =
        withheld_scenario_mass_status(bounded, reports_complete, vec![order_report], fill_reports);

    let (result, _) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());
    assert_eq!(count_filled_events(&result.events), 2);
    assert_eq!(
        ctx.cache
            .borrow()
            .position(&closing_scenario_position_id())
            .unwrap()
            .is_closed(),
        !covered && !warns
    );

    let warnings = withheld_fill_warnings(tag);

    if warns {
        assert_eq!(
            warnings,
            vec![format!(
                "Applied 2 bounded fill(s) for {} on {} to order state only, not to position {} \
                 (T-{tag}-1 ({closing_order_id}), T-{tag}-2 ({closing_order_id})): exposure and \
                 PnL may need manual reconciliation",
                test_instrument_id(),
                test_account_id(),
                closing_scenario_position_id(),
            )]
        );
    } else {
        assert!(warnings.is_empty(), "found {warnings:?}");
    }
}

#[tokio::test]
async fn test_bounded_filled_order_without_executions_warns() {
    let _log_guard = MANAGER_LOG_TEST_LOCK.lock().await;
    install_manager_log_capture();

    let tag = "WARN-INFERRED";
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    cache_closing_scenario_open_long(&ctx, &test_instrument());
    let (closing_order_id, reported_order_id) =
        prepare_closing_scenario_order_for(&mut ctx, ClosingOrder::Cached, tag);
    let (order_report, _) = withheld_scenario_reports(tag, reported_order_id);
    let mass_status = withheld_scenario_mass_status(true, true, vec![order_report], Vec::new());

    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());
    assert_eq!(count_filled_events(&result.events), 1);
    assert!(position_events.is_empty(), "found {position_events:?}");
    assert_closing_scenario_position_unchanged(&ctx.cache.borrow());

    let warnings = withheld_fill_warnings(&format!("({closing_order_id})"));
    assert_eq!(warnings.len(), 1, "found {warnings:?}");
    assert!(
        warnings[0].starts_with(&format!(
            "Applied 1 bounded fill(s) for {} on {} to order state only, not to position {} (",
            test_instrument_id(),
            test_account_id(),
            closing_scenario_position_id(),
        )),
        "found {warnings:?}"
    );
}

#[tokio::test]
async fn test_withheld_fill_warning_follows_coverage_per_instrument() {
    let _log_guard = MANAGER_LOG_TEST_LOCK.lock().await;
    install_manager_log_capture();

    let mut ctx = TestContext::with_cache(Cache::default(), ExecutionManagerConfig::default());

    {
        let mut engine = ctx.exec_engine.borrow_mut();
        engine.deregister_client(test_client_id()).unwrap();
        engine
            .register_client(Box::new(
                MockExecutionClient::new(Vec::new())
                    .with_uncovered_instrument(test_instrument_id()),
            ))
            .unwrap();
    }

    ctx.add_instrument(test_instrument());
    ctx.add_instrument(test_instrument2());
    register_closing_scenario_oms(&ctx);

    let (covered_report, covered_fill) = create_bounded_fill_lifecycle(
        test_instrument_id2(),
        VenueOrderId::from("V-WARN-COVERED"),
        TradeId::from("T-WARN-COVERED"),
        OrderSide::Buy,
        "1",
        "50000.0",
        false,
        UnixNanos::from(CLOSING_SCENARIO_FILL_TS_1),
    );
    let (uncovered_report, uncovered_fill) = create_bounded_fill_lifecycle(
        test_instrument_id(),
        VenueOrderId::from("V-WARN-UNCOVERED"),
        TradeId::from("T-WARN-UNCOVERED"),
        OrderSide::Buy,
        "1.000",
        "3000.00",
        false,
        UnixNanos::from(CLOSING_SCENARIO_FILL_TS_2),
    );
    let mass_status = withheld_scenario_mass_status(
        true,
        false,
        vec![covered_report, uncovered_report],
        vec![covered_fill, uncovered_fill],
    );

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(count_filled_events(&result.events), 2);
    assert!(withheld_fill_warnings("T-WARN-COVERED").is_empty());
    assert_eq!(
        withheld_fill_warnings("T-WARN-UNCOVERED"),
        vec![format!(
            "Applied 1 bounded fill(s) for {} on {} to order state only, with no resolvable \
             position (T-WARN-UNCOVERED (V-WARN-UNCOVERED)): exposure and PnL may need manual \
             reconciliation",
            test_instrument_id(),
            test_account_id(),
        )]
    );
}

#[tokio::test]
async fn test_withheld_fill_warning_omits_fill_rejected_by_projection() {
    let _log_guard = MANAGER_LOG_TEST_LOCK.lock().await;
    install_manager_log_capture();

    let tag = "WARN-OVERFILL";
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    cache_closing_scenario_open_long(&ctx, &test_instrument());
    let (closing_order_id, reported_order_id) =
        prepare_closing_scenario_order_for(&mut ctx, ClosingOrder::Cached, tag);
    let (_, mut fill_reports) = withheld_scenario_reports(tag, reported_order_id);
    let mut overfill = fill_reports[1].clone();
    overfill.trade_id = TradeId::new(format!("T-{tag}-3"));
    overfill.last_qty = Quantity::from("0.500");
    overfill.ts_event = UnixNanos::from(CLOSING_SCENARIO_FILL_TS_2 + 500_000);
    fill_reports.push(overfill);
    let mass_status = withheld_scenario_mass_status(true, false, Vec::new(), fill_reports);

    let (result, _) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert_eq!(count_filled_events(&result.events), 3);
    let order = ctx
        .get_order(&closing_order_id)
        .expect("closing order cached");
    assert_eq!(order.filled_qty(), Quantity::from("1.000"));
    assert_eq!(
        withheld_fill_warnings(tag),
        vec![format!(
            "Applied 2 bounded fill(s) for {} on {} to order state only, not to position {} \
             (T-{tag}-1 ({closing_order_id}), T-{tag}-2 ({closing_order_id})): exposure and \
             PnL may need manual reconciliation",
            test_instrument_id(),
            test_account_id(),
            closing_scenario_position_id(),
        )]
    );
}

#[tokio::test]
async fn test_bounded_fill_in_retained_state_does_not_warn() {
    let _log_guard = MANAGER_LOG_TEST_LOCK.lock().await;
    install_manager_log_capture();

    let tag = "WARN-RETAINED";
    let instrument = test_instrument();
    let venue_order_id = VenueOrderId::new(format!("V-{tag}"));
    let trade_id = TradeId::new(format!("T-{tag}-1"));
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);

    let mut retained_order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(ClientOrderId::new(format!("O-{tag}")))
        .strategy_id(closing_scenario_strategy_id())
        .instrument_id(test_instrument_id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .price(Price::from("3000.00"))
        .build();
    apply_submitted_and_accepted(&mut retained_order, venue_order_id);
    let retained_fill = TestOrderEventStubs::filled(
        &retained_order,
        &instrument,
        Some(trade_id),
        Some(closing_scenario_position_id()),
        Some(Price::from("3000.00")),
        Some(Quantity::from("1.000")),
        Some(LiquiditySide::Taker),
        Some(Money::from("0.20 USDT")),
        Some(UnixNanos::from(CLOSING_SCENARIO_FILL_TS_1)),
        Some(test_account_id()),
    );
    let position = Position::new(&instrument, retained_fill.into());
    ctx.cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();

    let (order_report, fill_report) = create_bounded_fill_lifecycle(
        test_instrument_id(),
        venue_order_id,
        trade_id,
        OrderSide::Buy,
        "1.000",
        "3000.00",
        false,
        UnixNanos::from(CLOSING_SCENARIO_FILL_TS_1),
    );
    let mass_status =
        withheld_scenario_mass_status(true, true, vec![order_report], vec![fill_report]);

    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass_status);

    assert!(result.unresolved_positions.is_empty());
    assert_eq!(count_filled_events(&result.events), 1);
    assert!(position_events.is_empty(), "found {position_events:?}");
    let cache = ctx.cache.borrow();
    let position = cache
        .position(&closing_scenario_position_id())
        .expect("retained position cached");
    assert_eq!(position.quantity, Quantity::from("1.000"));
    assert_eq!(position.events.len(), 1);
    drop(position);
    drop(cache);
    assert!(
        withheld_fill_warnings(tag).is_empty(),
        "found {:?}",
        MANAGER_LOG_CAPTURE.messages.lock()
    );
}

#[tokio::test]
async fn test_bounded_real_fill_after_restored_inference_stays_order_only() {
    let (closing_order_id, inferred_trade_id, restored) = {
        let mut ctx =
            closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
        cache_closing_scenario_open_long(&ctx, &test_instrument());
        let (closing_order_id, _) = prepare_closing_scenario_order(&mut ctx, ClosingOrder::Cached);
        let mut report = closing_scenario_closing_order_report(Some(closing_order_id));
        report.filled_qty = Quantity::from("0.400");
        report.order_status = OrderStatus::PartiallyFilled;
        report.avg_px = Some(dec!(3100));

        let mut mass = ExecutionMassStatus::new(
            test_client_id(),
            test_account_id(),
            test_venue(),
            UnixNanos::from(CLOSING_SCENARIO_FILL_TS_1),
            None,
        );

        mass.add_order_reports(vec![report]);
        let (result, _) = reconcile_closing_scenario(&mut ctx, &mass);

        let inferred_trade_id = result
            .events
            .iter()
            .find_map(|event| match event {
                OrderEventAny::Filled(fill) => Some(fill.trade_id),
                _ => None,
            })
            .unwrap();

        assert_eq!(
            ctx.cache
                .borrow()
                .position(&closing_scenario_position_id())
                .unwrap()
                .quantity,
            Quantity::from("0.600")
        );

        (
            closing_order_id,
            inferred_trade_id,
            restore_closing_cache(&ctx.cache.borrow()),
        )
    };

    let mut ctx = closing_scenario_context(restored, ExecutionManagerConfig::default(), false);

    let mut mass = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::from(CLOSING_SCENARIO_FILL_TS_2),
        None,
    );

    mass.set_report_window(Some(UnixNanos::from(CLOSING_SCENARIO_WINDOW_START)), true);
    let fill = closing_scenario_closing_fill_reports(Some(closing_order_id)).remove(0);
    mass.add_fill_reports(vec![fill]);

    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass);
    let cache = ctx.cache.borrow();
    let position = cache.position(&closing_scenario_position_id()).unwrap();
    assert_eq!(count_filled_events(&result.events), 1);
    assert!(position_events.is_empty());
    assert_eq!(position.quantity, Quantity::from("0.600"));
    assert_eq!(position.realized_pnl, Some(Money::from("39.40 USDT")));
    assert_eq!(position.commissions(), vec![Money::from("0.60 USDT")]);
    assert_eq!(
        position.trade_ids,
        [TradeId::from("T-OPEN"), inferred_trade_id]
            .into_iter()
            .collect::<AHashSet<_>>()
    );
    assert_eq!(
        closing_scenario_instrument_position_ids(&cache),
        vec![closing_scenario_position_id()]
    );
}

#[rstest]
#[case::canceled(OrderStatus::Canceled)]
#[case::expired(OrderStatus::Expired)]
fn test_bounded_terminal_closing_order_with_complete_fills_closes_position(
    #[case] status: OrderStatus,
) {
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    cache_closing_scenario_open_long(&ctx, &test_instrument());
    let order_id = ClientOrderId::from("O-CLOSE");
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(order_id)
        .strategy_id(closing_scenario_strategy_id())
        .instrument_id(test_instrument_id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from("2.000"))
        .price(Price::from("3100.00"))
        .build();
    apply_submitted_and_accepted(&mut order, closing_scenario_venue_order_id());
    ctx.add_order(order);
    let mut mass = closing_scenario_mass_status(true, Some(order_id));
    let mut report = closing_scenario_closing_order_report(Some(order_id));
    report.quantity = Quantity::from("2.000");
    report.order_status = status;
    mass.add_order_reports(vec![report]);

    let (_, events) = reconcile_closing_scenario(&mut ctx, &mass);

    let cache = ctx.cache.borrow();
    let order = cache.order(&order_id).unwrap();
    assert_eq!(order.status(), status);
    assert_eq!(order.filled_qty(), Quantity::from("1.000"));
    assert_eq!(order.avg_px(), Some(dec!(3112)));
    let position = cache.position(&closing_scenario_position_id()).unwrap();
    assert_eq!(position.side, PositionSide::Flat);
    assert_eq!(position.quantity, Quantity::from("0.000"));
    assert_eq!(position.realized_pnl, Some(Money::from("110.78 USDT")));
    assert_eq!(position.commissions(), vec![Money::from("1.22 USDT")]);
    assert_eq!(position.closing_order_id, Some(order_id));
    assert_eq!(
        position.trade_ids,
        ["T-OPEN", "T-CLOSE-1", "T-CLOSE-2"]
            .into_iter()
            .map(TradeId::from)
            .collect()
    );
    assert_eq!(
        closing_scenario_instrument_position_ids(&cache),
        vec![closing_scenario_position_id()]
    );
    assert_closing_scenario_position_events(&events);
}

#[rstest]
fn test_bounded_closing_recovery_applies_explicit_fill_without_inferred_remainder() {
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    cache_closing_scenario_open_long(&ctx, &test_instrument());
    let (closing_order_id, reported_order_id) =
        prepare_closing_scenario_order(&mut ctx, ClosingOrder::Claimed);

    let mut mass = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::from(CLOSING_SCENARIO_FILL_TS_2),
        None,
    );

    mass.set_report_window(Some(UnixNanos::from(CLOSING_SCENARIO_WINDOW_START)), true);
    let report = closing_scenario_closing_order_report(reported_order_id).with_avg_px(dec!(3112));
    let fill = closing_scenario_closing_fill_reports(reported_order_id).remove(0);
    mass.add_order_reports(vec![report]);
    mass.add_fill_reports(vec![fill]);

    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass);
    assert_eq!(count_filled_events(&result.events), 2);
    assert_eq!(position_events.len(), 1);
    assert!(matches!(
        position_events[0],
        PositionEvent::PositionChanged(_)
    ));
    let cache = ctx.cache.borrow();
    let order = cache.order(&closing_order_id).unwrap();
    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.filled_qty(), Quantity::from("1.000"));
    assert_eq!(order.trade_ids().len(), 2);
    let position = cache.position(&closing_scenario_position_id()).unwrap();
    assert_eq!(position.quantity, Quantity::from("0.600"));
    assert_eq!(position.realized_pnl, Some(Money::from("39.15 USDT")));
    assert_eq!(position.commissions(), vec![Money::from("0.85 USDT")]);
    assert_eq!(
        position.trade_ids,
        ["T-OPEN", "T-CLOSE-1"]
            .into_iter()
            .map(TradeId::from)
            .collect()
    );
    assert_eq!(
        closing_scenario_instrument_position_ids(&cache),
        vec![closing_scenario_position_id()]
    );
}

#[rstest]
#[case::cached(ClosingOrder::Cached)]
#[case::claimed(ClosingOrder::Claimed)]
fn test_bounded_closing_fills_close_retained_short(#[case] closing: ClosingOrder) {
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    cache_closing_scenario_open_position(
        &ctx,
        &test_instrument(),
        test_account_id(),
        OmsType::Netting,
        OrderSide::Sell,
    );

    let reported_id = if matches!(closing, ClosingOrder::Cached) {
        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .client_order_id(ClientOrderId::from("O-CLOSE"))
            .strategy_id(closing_scenario_strategy_id())
            .instrument_id(test_instrument_id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.000"))
            .price(Price::from("2900.00"))
            .build();
        apply_submitted_and_accepted(&mut order, closing_scenario_venue_order_id());
        ctx.add_order(order);
        Some(ClientOrderId::from("O-CLOSE"))
    } else {
        claim_closing_scenario_instrument(&mut ctx, closing);
        None
    };

    let mut report = closing_scenario_closing_order_report(reported_id);
    report.order_side = Some(OrderSide::Buy);
    report.avg_px = Some(dec!(2888));
    let mut fills = closing_scenario_closing_fill_reports(reported_id);
    fills[0].order_side = OrderSide::Buy;
    fills[0].last_px = Price::from("2900.00");
    fills[1].order_side = OrderSide::Buy;
    fills[1].last_px = Price::from("2880.00");

    let mut mass = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::from(CLOSING_SCENARIO_FILL_TS_2),
        None,
    );

    mass.set_report_window(Some(UnixNanos::from(CLOSING_SCENARIO_WINDOW_START)), true);
    mass.add_order_reports(vec![report]);
    mass.add_fill_reports(fills);

    let (_, position_events) = reconcile_closing_scenario(&mut ctx, &mass);
    let cache = ctx.cache.borrow();
    let order_id = reported_id
        .unwrap_or_else(|| ClientOrderId::from(closing_scenario_venue_order_id().as_str()));
    assert_closing_scenario_recorded_once(&cache, order_id);
    assert_closing_scenario_position_events(&position_events);
}

#[rstest]
#[case::cached(ClosingOrder::Cached)]
#[case::claimed(ClosingOrder::Claimed)]
fn test_bounded_closing_recovery_does_not_apply_mismatched_instrument_fill(
    #[case] closing: ClosingOrder,
) {
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    cache_closing_scenario_open_long(&ctx, &test_instrument());
    let (closing_order_id, reported_order_id) = prepare_closing_scenario_order(&mut ctx, closing);
    let mut fills = closing_scenario_closing_fill_reports(reported_order_id);
    fills[1].instrument_id = test_instrument_id2();

    let mut mass = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::from(CLOSING_SCENARIO_FILL_TS_2),
        None,
    );

    mass.set_report_window(Some(UnixNanos::from(CLOSING_SCENARIO_WINDOW_START)), true);
    mass.add_order_reports(vec![closing_scenario_closing_order_report(
        reported_order_id,
    )]);
    mass.add_fill_reports(fills);

    let (_, position_events) = reconcile_closing_scenario(&mut ctx, &mass);
    assert_eq!(position_events.len(), 1);
    assert!(matches!(
        position_events[0],
        PositionEvent::PositionChanged(_)
    ));
    let cache = ctx.cache.borrow();
    assert_closing_scenario_order_filled(&cache, closing_order_id);
    let position = cache.position(&closing_scenario_position_id()).unwrap();
    assert_eq!(position.quantity, Quantity::from("0.600"));
    assert_eq!(position.realized_pnl, Some(Money::from("39.15 USDT")));
    assert_eq!(position.commissions(), vec![Money::from("0.85 USDT")]);
    assert_eq!(
        position.trade_ids,
        ["T-OPEN", "T-CLOSE-1"]
            .into_iter()
            .map(TradeId::from)
            .collect()
    );
}

#[rstest]
#[case::cached(ClosingOrder::Cached)]
#[case::claimed(ClosingOrder::Claimed)]
#[tokio::test]
async fn test_bounded_partial_close_then_remaining_fills_after_restore(
    #[case] closing: ClosingOrder,
) {
    let (closing_order_id, reported_order_id, restored) = {
        let mut ctx =
            closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
        cache_closing_scenario_open_long(&ctx, &test_instrument());
        let (closing_order_id, reported_order_id) =
            prepare_closing_scenario_order(&mut ctx, closing);
        let mut report = closing_scenario_closing_order_report(reported_order_id);
        report.order_status = OrderStatus::PartiallyFilled;
        report.filled_qty = Quantity::from("0.400");
        report.avg_px = Some(dec!(3100));

        let mut mass = ExecutionMassStatus::new(
            test_client_id(),
            test_account_id(),
            test_venue(),
            UnixNanos::from(CLOSING_SCENARIO_FILL_TS_1),
            None,
        );

        mass.set_report_window(Some(UnixNanos::from(CLOSING_SCENARIO_WINDOW_START)), true);
        mass.add_order_reports(vec![report]);
        mass.add_fill_reports(vec![
            closing_scenario_closing_fill_reports(reported_order_id).remove(0),
        ]);
        let (_, position_events) = reconcile_closing_scenario(&mut ctx, &mass);
        let cache = ctx.cache.borrow();
        let position = cache.position(&closing_scenario_position_id()).unwrap();
        assert_eq!(position.quantity, Quantity::from("0.600"));
        assert_eq!(position.realized_pnl, Some(Money::from("39.15 USDT")));
        assert_eq!(position.commissions(), vec![Money::from("0.85 USDT")]);
        assert_eq!(position_events.len(), 1);

        (
            closing_order_id,
            reported_order_id,
            restore_closing_cache(&ctx.cache.borrow()),
        )
    };

    let mut ctx = closing_scenario_context(restored, ExecutionManagerConfig::default(), false);
    claim_closing_scenario_instrument(&mut ctx, closing);
    let mass = closing_scenario_mass_status(true, reported_order_id);
    let (result, position_events) = reconcile_closing_scenario(&mut ctx, &mass);
    assert_eq!(count_filled_events(&result.events), 1);
    assert_eq!(position_events.len(), 1);
    assert!(matches!(
        position_events[0],
        PositionEvent::PositionClosed(_)
    ));
    assert_closing_scenario_recorded_once(&ctx.cache.borrow(), closing_order_id);

    let (replay, position_events) = reconcile_closing_scenario(&mut ctx, &mass);
    assert_eq!(count_filled_events(&replay.events), 0);
    assert!(position_events.is_empty());
    assert_closing_scenario_recorded_once(&ctx.cache.borrow(), closing_order_id);
}

#[rstest]
fn test_bounded_closing_fills_recheck_position_after_each_application() {
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    cache_closing_scenario_open_long(&ctx, &test_instrument());
    let (order_id, reported_id) = prepare_closing_scenario_order(&mut ctx, ClosingOrder::Cached);
    let other_id = PositionId::from("OTHER-POSITION");
    let other_order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(ClientOrderId::from("O-OTHER"))
        .strategy_id(closing_scenario_strategy_id())
        .instrument_id(test_instrument_id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("0.200"))
        .price(Price::from("3000.00"))
        .build();
    let fill = TestOrderEventStubs::filled(
        &other_order,
        &test_instrument(),
        Some(TradeId::from("T-OTHER")),
        Some(other_id),
        Some(Price::from("3000.00")),
        Some(Quantity::from("0.200")),
        Some(LiquiditySide::Maker),
        Some(Money::from("0.12 USDT")),
        Some(UnixNanos::from(CLOSING_SCENARIO_OPENING_TS)),
        Some(test_account_id()),
    );
    let other = Position::new(&test_instrument(), fill.into());
    ctx.cache
        .borrow_mut()
        .add_position(&other, OmsType::Netting)
        .unwrap();

    let handler = TypedHandler::from({
        let cache = ctx.cache.clone();
        move |_: &PositionEvent| {
            cache
                .borrow_mut()
                .add_position_id(
                    &other_id,
                    &test_venue(),
                    &order_id,
                    &closing_scenario_strategy_id(),
                )
                .unwrap();
        }
    });

    msgbus::subscribe_position_events("events.position.*".into(), handler.clone(), None);
    let mass = closing_scenario_mass_status(true, reported_id);
    let (_, events) = reconcile_closing_scenario(&mut ctx, &mass);
    msgbus::unsubscribe_position_events("events.position.*".into(), &handler);

    let cache = ctx.cache.borrow();
    let position = cache.position(&closing_scenario_position_id()).unwrap();
    assert_eq!(position.quantity, Quantity::from("0.600"));
    assert_eq!(position.realized_pnl, Some(Money::from("39.15 USDT")));
    assert_eq!(position.commissions(), vec![Money::from("0.85 USDT")]);
    let other = cache.position(&other_id).unwrap();
    assert_eq!(other.quantity, Quantity::from("0.200"));
    assert_eq!(other.realized_pnl, Some(Money::from("-0.12 USDT")));
    assert_eq!(other.commissions(), vec![Money::from("0.12 USDT")]);
    assert_eq!(events.len(), 1);
    assert_closing_scenario_order_filled(&cache, order_id);
}

#[rstest]
fn test_bounded_closing_fills_respect_target_lifecycle_across_strategies() {
    let mut ctx =
        closing_scenario_context(Cache::default(), ExecutionManagerConfig::default(), false);
    cache_closing_scenario_open_long(&ctx, &test_instrument());
    let (order_id, reported_id) = prepare_closing_scenario_order(&mut ctx, ClosingOrder::Cached);
    let external_id = PositionId::new(format!("{}-EXTERNAL", test_instrument_id()));

    {
        let mut cache = ctx.cache.borrow_mut();
        let mut position = cache
            .position_owned(&closing_scenario_position_id())
            .unwrap();
        cache.purge_position(position.id, false).unwrap();
        position.id = external_id;
        position.strategy_id = StrategyId::external();
        position.ts_opened = UnixNanos::from(CLOSING_SCENARIO_FILL_TS_2 + 1);
        cache.add_position(&position, OmsType::Netting).unwrap();
        cache
            .add_position_id(
                &external_id,
                &test_venue(),
                &order_id,
                &closing_scenario_strategy_id(),
            )
            .unwrap();
    }

    let mass = closing_scenario_mass_status(true, reported_id);
    let (_, events) = reconcile_closing_scenario(&mut ctx, &mass);

    let cache = ctx.cache.borrow();
    let position = cache.position(&external_id).unwrap();
    assert_eq!(position.quantity, Quantity::from("1.000"));
    assert_eq!(position.realized_pnl, Some(Money::from("-0.60 USDT")));
    assert_eq!(position.commissions(), vec![Money::from("0.60 USDT")]);
    assert_eq!(
        position.trade_ids,
        [TradeId::from("T-OPEN")].into_iter().collect()
    );
    assert!(events.is_empty());
    assert_closing_scenario_order_filled(&cache, order_id);
}

fn restore_closing_cache(cache: &Cache) -> Cache {
    let orders: Vec<_> = cache
        .orders(None, None, None, None, None)
        .into_iter()
        .map(|order| {
            (
                order.clone(),
                cache.position_id(&order.client_order_id()).copied(),
                cache.client_id(&order.client_order_id()).copied(),
            )
        })
        .collect();

    let positions: Vec<_> = cache
        .positions(None, None, None, None, None)
        .into_iter()
        .map(|position| (position.clone(), cache.oms_type(&position.id).unwrap()))
        .collect();
    let orders = serde_json::to_vec(&orders).unwrap();
    let orders: Vec<(OrderAny, Option<PositionId>, Option<ClientId>)> =
        serde_json::from_slice(&orders).unwrap();
    let positions = serde_json::to_vec(&positions).unwrap();
    let positions: Vec<(Position, OmsType)> = serde_json::from_slice(&positions).unwrap();
    let mut restored = Cache::default();
    for (order, position_id, client_id) in orders {
        restored
            .add_order(order, position_id, client_id, false)
            .unwrap();
    }

    for (position, oms_type) in positions {
        restored.add_position(&position, oms_type).unwrap();
    }

    restored.build_index();
    restored
}

const CLOSING_SCENARIO_TAG: &str = "CLOSE";
const CLOSING_SCENARIO_OPENING_TS: u64 = 1_000_000_000_000;
const CLOSING_SCENARIO_WINDOW_START: u64 = 2_000_000_000_000;
const CLOSING_SCENARIO_ACCEPTED_TS: u64 = 2_000_000_500_000;
const CLOSING_SCENARIO_FILL_TS_1: u64 = 2_000_001_000_000;
const CLOSING_SCENARIO_FILL_TS_2: u64 = 2_000_002_000_000;

fn closing_scenario_strategy_id() -> StrategyId {
    StrategyId::from("STRATEGY-001")
}

fn closing_scenario_position_id() -> PositionId {
    PositionId::new(format!(
        "{}-{}",
        test_instrument_id(),
        closing_scenario_strategy_id()
    ))
}

fn closing_scenario_venue_order_id() -> VenueOrderId {
    VenueOrderId::new(format!("V-{CLOSING_SCENARIO_TAG}"))
}

fn register_closing_scenario_oms(ctx: &TestContext) {
    let mut engine = ctx.exec_engine.borrow_mut();
    engine.register_oms_type(closing_scenario_strategy_id(), OmsType::Netting);
    engine.register_oms_type(StrategyId::from("EXTERNAL"), OmsType::Netting);
}

fn cache_closing_scenario_open_long(ctx: &TestContext, instrument: &InstrumentAny) {
    cache_closing_scenario_open_position(
        ctx,
        instrument,
        test_account_id(),
        OmsType::Netting,
        OrderSide::Buy,
    );
}

fn cache_closing_scenario_open_position(
    ctx: &TestContext,
    instrument: &InstrumentAny,
    account_id: AccountId,
    oms_type: OmsType,
    opening_side: OrderSide,
) {
    let position_id = closing_scenario_position_id();
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(ClientOrderId::from("O-OPEN"))
        .strategy_id(closing_scenario_strategy_id())
        .instrument_id(instrument.id())
        .side(opening_side)
        .quantity(Quantity::from("1.000"))
        .price(Price::from("3000.00"))
        .build();
    apply_submitted_and_accepted(&mut order, VenueOrderId::from("V-OPEN"));
    let filled = TestOrderEventStubs::filled(
        &order,
        instrument,
        Some(TradeId::from("T-OPEN")),
        Some(position_id),
        Some(Price::from("3000.00")),
        Some(Quantity::from("1.000")),
        Some(LiquiditySide::Maker),
        Some(Money::from("0.60 USDT")),
        Some(UnixNanos::from(CLOSING_SCENARIO_OPENING_TS)),
        Some(account_id),
    );
    order.apply(filled.clone()).unwrap();
    let position = Position::new(instrument, filled.into());
    assert_eq!(position.realized_pnl, Some(Money::from("-0.60 USDT")));

    let mut cache = ctx.cache.borrow_mut();
    cache
        .add_order(order, Some(position_id), Some(test_client_id()), false)
        .unwrap();
    cache.add_position(&position, oms_type).unwrap();
}

fn closing_scenario_opening_order_report() -> OrderStatusReport {
    let ts = UnixNanos::from(CLOSING_SCENARIO_OPENING_TS);
    OrderStatusReport::new(
        test_account_id(),
        test_instrument_id(),
        Some(ClientOrderId::from("O-OPEN")),
        VenueOrderId::from("V-OPEN"),
        OrderSide::Buy.into(),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Filled,
        Quantity::from("1.000"),
        Quantity::from("1.000"),
        ts,
        ts,
        ts,
        None,
    )
    .with_price(Price::from("3000.00"))
    .with_avg_px(dec!(3000.00))
}

fn closing_scenario_opening_fill_report() -> FillReport {
    let ts = UnixNanos::from(CLOSING_SCENARIO_OPENING_TS);

    FillReport::new(
        test_account_id(),
        test_instrument_id(),
        VenueOrderId::from("V-OPEN"),
        TradeId::from("T-OPEN"),
        OrderSide::Buy,
        Quantity::from("1.000"),
        Price::from("3000.00"),
        Money::from("0.60 USDT"),
        LiquiditySide::Maker,
        Some(ClientOrderId::from("O-OPEN")),
        None,
        ts,
        ts,
        None,
    )
}

fn closing_scenario_closing_order_report(
    client_order_id: Option<ClientOrderId>,
) -> OrderStatusReport {
    OrderStatusReport::new(
        test_account_id(),
        test_instrument_id(),
        client_order_id,
        closing_scenario_venue_order_id(),
        OrderSide::Sell.into(),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Filled,
        Quantity::from("1.000"),
        Quantity::from("1.000"),
        UnixNanos::from(CLOSING_SCENARIO_ACCEPTED_TS),
        UnixNanos::from(CLOSING_SCENARIO_FILL_TS_2),
        UnixNanos::from(CLOSING_SCENARIO_FILL_TS_2),
        None,
    )
    .with_price(Price::from("3100.00"))
    .with_avg_px(dec!(3112.00))
}

fn closing_scenario_closing_fill_reports(
    client_order_id: Option<ClientOrderId>,
) -> Vec<FillReport> {
    [
        (
            "T-CLOSE-1",
            "0.400",
            "3100.00",
            "0.25 USDT",
            CLOSING_SCENARIO_FILL_TS_1,
        ),
        (
            "T-CLOSE-2",
            "0.600",
            "3120.00",
            "0.37 USDT",
            CLOSING_SCENARIO_FILL_TS_2,
        ),
    ]
    .into_iter()
    .map(|(trade_id, quantity, price, commission, ts)| {
        FillReport::new(
            test_account_id(),
            test_instrument_id(),
            closing_scenario_venue_order_id(),
            TradeId::from(trade_id),
            OrderSide::Sell,
            Quantity::from(quantity),
            Price::from(price),
            Money::from(commission),
            LiquiditySide::Taker,
            client_order_id,
            None,
            UnixNanos::from(ts),
            UnixNanos::from(ts),
            None,
        )
    })
    .collect()
}

fn closing_scenario_mass_status(
    bounded: bool,
    closing_client_order_id: Option<ClientOrderId>,
) -> ExecutionMassStatus {
    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::from(CLOSING_SCENARIO_FILL_TS_2 + 1_000_000),
        Some(UUID4::new()),
    );

    let mut order_reports = vec![closing_scenario_closing_order_report(
        closing_client_order_id,
    )];
    let mut fill_reports = closing_scenario_closing_fill_reports(closing_client_order_id);

    if bounded {
        mass_status.set_report_window(Some(UnixNanos::from(CLOSING_SCENARIO_WINDOW_START)), true);
    } else {
        order_reports.insert(0, closing_scenario_opening_order_report());
        fill_reports.insert(0, closing_scenario_opening_fill_report());
    }

    mass_status.add_order_reports(order_reports);
    mass_status.add_fill_reports(fill_reports);
    mass_status
}

fn capture_position_events() -> (Rc<RefCell<Vec<PositionEvent>>>, TypedHandler<PositionEvent>) {
    let received = Rc::new(RefCell::new(Vec::<PositionEvent>::new()));

    let handler = TypedHandler::from({
        let received = received.clone();
        move |event: &PositionEvent| received.borrow_mut().push(event.clone())
    });

    msgbus::subscribe_position_events("events.position.*".into(), handler.clone(), None);
    (received, handler)
}

fn release_position_events(handler: &TypedHandler<PositionEvent>) {
    msgbus::unsubscribe_position_events("events.position.*".into(), handler);
}

fn count_filled_events(events: &[OrderEventAny]) -> usize {
    events
        .iter()
        .filter(|event| matches!(event, OrderEventAny::Filled(_)))
        .count()
}

fn assert_closing_scenario_recorded_once(cache: &Cache, closing_order_id: ClientOrderId) {
    let instrument_id = test_instrument_id();
    let account_id = test_account_id();

    assert_closing_scenario_order_filled(cache, closing_order_id);

    let position_ids = cache
        .positions(None, Some(&instrument_id), None, Some(&account_id), None)
        .iter()
        .map(|position| position.id)
        .collect::<Vec<_>>();
    assert_eq!(
        position_ids,
        vec![closing_scenario_position_id()],
        "a closing fill must not open an opposite position"
    );
    let position = cache
        .position(&closing_scenario_position_id())
        .expect("restored position cached");
    assert!(
        position.is_closed(),
        "restored position must be closed by its closing fills, found side {:?} quantity {} realized_pnl {:?} trade_ids {:?}",
        position.side,
        position.quantity,
        position.realized_pnl,
        position.trade_ids,
    );
    assert_eq!(position.side, PositionSide::Flat);
    assert_eq!(position.quantity, Quantity::from("0.000"));
    assert_eq!(position.signed_decimal_qty(), dec!(0));
    assert_eq!(position.realized_pnl, Some(Money::from("110.78 USDT")));
    assert_eq!(position.commissions(), vec![Money::from("1.22 USDT")]);
    assert_eq!(
        position.trade_ids,
        [
            TradeId::from("T-OPEN"),
            TradeId::from("T-CLOSE-1"),
            TradeId::from("T-CLOSE-2"),
        ]
        .into_iter()
        .collect::<AHashSet<_>>()
    );
    assert_eq!(position.events.len(), 3);
    assert_eq!(position.closing_order_id, Some(closing_order_id));
    assert_eq!(
        position.ts_closed,
        Some(UnixNanos::from(CLOSING_SCENARIO_FILL_TS_2))
    );
    drop(position);

    assert_eq!(
        cache
            .positions_open(None, Some(&instrument_id), None, Some(&account_id), None)
            .len(),
        0
    );
    assert_eq!(
        cache
            .positions_closed(None, Some(&instrument_id), None, Some(&account_id), None)
            .len(),
        1
    );
}

fn assert_closing_scenario_position_events(events: &[PositionEvent]) {
    assert_eq!(
        events.len(),
        2,
        "expected PositionChanged then PositionClosed, found {events:?}"
    );
    assert!(matches!(events[0], PositionEvent::PositionChanged(_)));
    assert!(matches!(events[1], PositionEvent::PositionClosed(_)));
}

fn assert_closing_scenario_order_filled(cache: &Cache, closing_order_id: ClientOrderId) {
    let order = cache
        .order(&closing_order_id)
        .expect("closing order cached");
    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.filled_qty(), Quantity::from("1.000"));
    assert_eq!(
        order.trade_ids().into_iter().copied().collect::<Vec<_>>(),
        vec![TradeId::from("T-CLOSE-1"), TradeId::from("T-CLOSE-2")]
    );
}

fn assert_closing_scenario_position_unchanged(cache: &Cache) {
    let position = cache
        .position(&closing_scenario_position_id())
        .expect("restored position cached");
    assert!(position.is_open());
    assert_eq!(position.side, PositionSide::Long);
    assert_eq!(position.quantity, Quantity::from("1.000"));
    assert_eq!(position.realized_pnl, Some(Money::from("-0.60 USDT")));
    assert_eq!(position.commissions(), vec![Money::from("0.60 USDT")]);
    assert_eq!(
        position.trade_ids,
        [TradeId::from("T-OPEN")]
            .into_iter()
            .collect::<AHashSet<_>>()
    );
    assert_eq!(position.events.len(), 1);
}

fn closing_scenario_instrument_position_ids(cache: &Cache) -> Vec<PositionId> {
    let mut position_ids: Vec<PositionId> = cache
        .positions(None, Some(&test_instrument_id()), None, None, None)
        .iter()
        .map(|position| position.id)
        .collect();
    position_ids.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    position_ids
}

fn assert_closing_scenario_order_only(cache: &Cache, closing_order_id: ClientOrderId) {
    assert_closing_scenario_order_filled(cache, closing_order_id);
    assert_closing_scenario_position_unchanged(cache);
    assert_eq!(
        closing_scenario_instrument_position_ids(cache),
        vec![closing_scenario_position_id()]
    );
}

#[derive(Clone, Copy, Debug)]
enum ClosingOrder {
    // The strategy's order `O-CLOSE` is cached before reconciliation.
    Cached,
    // The order is placed outside the node and its instrument is claimed for the strategy.
    Claimed,
    // The order is placed outside the node and its instrument is not claimed.
    Unclaimed,
}

fn closing_scenario_context(
    cache: Cache,
    config: ExecutionManagerConfig,
    covered: bool,
) -> TestContext {
    let ctx = TestContext::with_cache(cache, config);

    {
        let mut engine = ctx.exec_engine.borrow_mut();
        engine.deregister_client(test_client_id()).unwrap();
        engine
            .register_client(Box::new(
                MockExecutionClient::new(Vec::new()).with_bulk_position_coverage(covered),
            ))
            .unwrap();
    }

    ctx.add_instrument(test_instrument());
    register_closing_scenario_oms(&ctx);
    ctx
}

fn claim_closing_scenario_instrument(ctx: &mut TestContext, closing: ClosingOrder) {
    if matches!(closing, ClosingOrder::Claimed) {
        ctx.manager
            .claim_external_orders(test_instrument_id(), closing_scenario_strategy_id())
            .unwrap();
    }
}

fn prepare_closing_scenario_order(
    ctx: &mut TestContext,
    closing: ClosingOrder,
) -> (ClientOrderId, Option<ClientOrderId>) {
    prepare_closing_scenario_order_for(ctx, closing, CLOSING_SCENARIO_TAG)
}

fn prepare_closing_scenario_order_for(
    ctx: &mut TestContext,
    closing: ClosingOrder,
    tag: &str,
) -> (ClientOrderId, Option<ClientOrderId>) {
    let venue_order_id = VenueOrderId::new(format!("V-{tag}"));

    match closing {
        ClosingOrder::Cached => {
            let client_order_id = ClientOrderId::new(format!("O-{tag}"));
            let mut order = OrderTestBuilder::new(OrderType::Limit)
                .client_order_id(client_order_id)
                .strategy_id(closing_scenario_strategy_id())
                .instrument_id(test_instrument_id())
                .side(OrderSide::Sell)
                .quantity(Quantity::from("1.000"))
                .price(Price::from("3100.00"))
                .build();
            apply_submitted_and_accepted(&mut order, venue_order_id);
            ctx.add_order(order);
            (client_order_id, Some(client_order_id))
        }
        ClosingOrder::Claimed | ClosingOrder::Unclaimed => {
            claim_closing_scenario_instrument(ctx, closing);
            (ClientOrderId::from(venue_order_id.as_str()), None)
        }
    }
}

fn reconcile_closing_scenario(
    ctx: &mut TestContext,
    mass_status: &ExecutionMassStatus,
) -> (ReconciliationResult, Vec<PositionEvent>) {
    let (position_events, handler) = capture_position_events();
    let result = ctx
        .manager
        .reconcile_execution_mass_status(mass_status, &ctx.exec_engine);
    release_position_events(&handler);
    let position_events = position_events.borrow().clone();
    (result, position_events)
}

fn withheld_scenario_reports(
    tag: &str,
    client_order_id: Option<ClientOrderId>,
) -> (OrderStatusReport, Vec<FillReport>) {
    let venue_order_id = VenueOrderId::new(format!("V-{tag}"));
    let mut order_report = closing_scenario_closing_order_report(client_order_id);
    order_report.venue_order_id = venue_order_id;

    let fill_reports = closing_scenario_closing_fill_reports(client_order_id)
        .into_iter()
        .enumerate()
        .map(|(index, mut fill)| {
            fill.venue_order_id = venue_order_id;
            fill.trade_id = TradeId::new(format!("T-{tag}-{}", index + 1));
            fill
        })
        .collect();

    (order_report, fill_reports)
}

fn withheld_scenario_mass_status(
    bounded: bool,
    reports_complete: bool,
    order_reports: Vec<OrderStatusReport>,
    fill_reports: Vec<FillReport>,
) -> ExecutionMassStatus {
    let mut mass_status = create_mass_status(order_reports, fill_reports);
    mass_status.ts_init = UnixNanos::from(CLOSING_SCENARIO_FILL_TS_2 + 1_000_000);
    if bounded {
        mass_status.set_report_window(
            Some(UnixNanos::from(CLOSING_SCENARIO_WINDOW_START)),
            reports_complete,
        );
    }

    mass_status
}

fn withheld_fill_warnings(needle: &str) -> Vec<String> {
    MANAGER_LOG_CAPTURE
        .messages
        .lock()
        .iter()
        .filter(|message| message.contains("to order state only") && message.contains(needle))
        .cloned()
        .collect()
}

#[derive(Clone, Copy)]
enum OtoRecoverySource {
    RetainedSnapshot { inferred: bool },
    LateStandalone,
    ReplacementSnapshot(ReplacementParentFill),
}

#[derive(Clone, Copy)]
enum ReplacementParentFill {
    WithOrderReport,
    CacheLinked,
    PriorLifecycle,
    DelayedPriorLifecycle,
}

#[rstest]
#[case::retained_snapshot(
    OtoRecoverySource::RetainedSnapshot { inferred: false },
    OrderStatus::Emulated
)]
#[case::retained_snapshot_inferred_fill(
    OtoRecoverySource::RetainedSnapshot { inferred: true },
    OrderStatus::Emulated
)]
#[case::late_standalone(OtoRecoverySource::LateStandalone, OrderStatus::Canceled)]
#[case::replacement_snapshot(
    OtoRecoverySource::ReplacementSnapshot(ReplacementParentFill::WithOrderReport),
    OrderStatus::Emulated
)]
#[case::cache_linked_replacement_fill(
    OtoRecoverySource::ReplacementSnapshot(ReplacementParentFill::CacheLinked),
    OrderStatus::Emulated
)]
#[case::before_replacement_snapshot(
    OtoRecoverySource::ReplacementSnapshot(ReplacementParentFill::PriorLifecycle),
    OrderStatus::Canceled
)]
#[case::late_fill_before_replacement_snapshot(
    OtoRecoverySource::ReplacementSnapshot(ReplacementParentFill::DelayedPriorLifecycle),
    OrderStatus::Canceled
)]
#[tokio::test]
async fn test_reconciled_oto_parent_exit_recovers_only_in_its_position_lifecycle(
    #[case] source: OtoRecoverySource,
    #[case] expected_status: OrderStatus,
) {
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let strategy_id = StrategyId::from("STRATEGY-001");
    let position_id = PositionId::new(format!("{instrument_id}-{strategy_id}"));
    let parent_id = ClientOrderId::from("O-OTO-PARENT");
    let parent_venue_order_id = VenueOrderId::from("V-OTO-PARENT");
    let child_id = ClientOrderId::from("O-OTO-EXIT");
    let mut trade_id = TradeId::from("T-OTO-PARENT");
    let qty = Quantity::from("1.000");
    let price = Price::from("3000.00");
    ctx.add_instrument(instrument.clone());
    ctx.manager
        .claim_external_orders(instrument_id, strategy_id)
        .unwrap();
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(strategy_id, OmsType::Netting);

    let mut parent = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(parent_id)
        .strategy_id(strategy_id)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(qty)
        .price(price)
        .contingency_type(ContingencyType::Oto)
        .linked_order_ids(vec![child_id])
        .build();
    apply_submitted_and_accepted(&mut parent, parent_venue_order_id);
    ctx.add_order(parent.clone());

    let mut child = OrderTestBuilder::new(OrderType::StopMarket)
        .client_order_id(child_id)
        .strategy_id(strategy_id)
        .instrument_id(instrument_id)
        .parent_order_id(parent_id)
        .side(OrderSide::Sell)
        .quantity(qty)
        .trigger_price(Price::from("2900.00"))
        .emulation_trigger(TriggerType::BidAsk)
        .reduce_only(true)
        .build();
    child
        .apply(OrderEventAny::Emulated(
            OrderEmulatedSpec::builder()
                .trader_id(child.trader_id())
                .strategy_id(strategy_id)
                .instrument_id(instrument_id)
                .client_order_id(child_id)
                .build(),
        ))
        .unwrap();
    ctx.add_order(child);

    let mut report = create_fill_report(
        parent_id,
        parent_venue_order_id,
        instrument_id,
        trade_id,
        "1.000",
    );
    let order_report = create_order_report(
        Some(parent_id),
        parent_venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        qty,
        qty,
    )
    .with_avg_px(dec!(3000));

    let expected_qty = match source {
        OtoRecoverySource::RetainedSnapshot { inferred } => {
            cache_oto_recovery_snapshot(&ctx, &instrument, strategy_id, position_id, true);

            let fills = if inferred {
                trade_id = create_inferred_reconciliation_trade_id(
                    test_account_id(),
                    instrument_id,
                    parent_id,
                    Some(parent_venue_order_id),
                    OrderSide::Buy,
                    OrderType::Limit,
                    qty,
                    qty,
                    price,
                    PositionId::new(format!("{instrument_id}-EXTERNAL")),
                    order_report.ts_last,
                );
                vec![]
            } else {
                vec![report]
            };

            ctx.manager.reconcile_execution_mass_status(
                &create_mass_status(vec![order_report], fills),
                &ctx.exec_engine,
            );
            dec!(1)
        }
        OtoRecoverySource::LateStandalone => {
            cache_oto_recovery_snapshot(&ctx, &instrument, strategy_id, position_id, false);
            ctx.exec_engine
                .borrow_mut()
                .reconcile_execution_report(&ExecutionReport::Fill(Box::new(report)));
            dec!(2)
        }
        OtoRecoverySource::ReplacementSnapshot(source) => {
            let earlier = |id: &str, side, ts| {
                FillReport::new(
                    test_account_id(),
                    instrument_id,
                    VenueOrderId::from(id),
                    TradeId::from(id),
                    side,
                    qty,
                    price,
                    Money::from("0.00 USDT"),
                    LiquiditySide::Maker,
                    None,
                    None,
                    UnixNanos::from(ts),
                    UnixNanos::from(ts),
                    None,
                )
            };

            let delayed = matches!(source, ReplacementParentFill::DelayedPriorLifecycle);

            let other_buy = if matches!(
                source,
                ReplacementParentFill::PriorLifecycle
                    | ReplacementParentFill::DelayedPriorLifecycle
            ) {
                earlier("V-OTHER-NEW-BUY", OrderSide::Buy, 3_000_000_u64)
            } else {
                report.ts_event = 3_000_000.into();
                report.ts_init = report.ts_event;
                earlier("V-OTHER-OLD-BUY", OrderSide::Buy, 1_000_000_u64)
            };

            let flat = earlier("V-FLAT", OrderSide::Sell, 2_000_000_u64);

            let order_reports = match source {
                ReplacementParentFill::WithOrderReport | ReplacementParentFill::PriorLifecycle => {
                    vec![order_report]
                }
                ReplacementParentFill::CacheLinked
                | ReplacementParentFill::DelayedPriorLifecycle => {
                    vec![]
                }
            };

            let batch_fill = if delayed {
                earlier("V-OTHER-OLD-BUY", OrderSide::Buy, 1_000_000_u64)
            } else {
                report.clone()
            };

            let mut mass = create_mass_status(order_reports, vec![batch_fill, other_buy, flat]);
            mass.add_position_reports(vec![PositionStatusReport::new(
                test_account_id(),
                instrument_id,
                PositionSide::Long,
                Quantity::from("2.000"),
                4_000_000.into(),
                4_000_000.into(),
                None,
                None,
                Some(dec!(3000)),
            )]);
            let result = ctx
                .manager
                .reconcile_execution_mass_status(&mass, &ctx.exec_engine);
            assert!(result.unresolved_positions.is_empty());

            if delayed {
                ctx.exec_engine
                    .borrow_mut()
                    .reconcile_execution_report(&ExecutionReport::Fill(Box::new(report)));
            }

            dec!(2)
        }
    };

    let mut emulator = OrderEmulator::new(ctx.clock.clone(), ctx.cache.clone());
    emulator.on_start().unwrap();

    let cache = ctx.cache.borrow();
    let recovered_parent = cache.order(&parent_id).unwrap();
    assert_eq!(recovered_parent.status(), OrderStatus::Filled);
    assert_eq!(recovered_parent.trade_ids(), vec![&trade_id]);
    let position = cache.position_ref(&position_id).unwrap();
    assert_eq!(position.signed_decimal_qty(), expected_qty);
    assert!(!position.trade_ids.contains(&trade_id));
    assert_eq!(cache.order(&child_id).unwrap().status(), expected_status);
    assert_eq!(
        emulator
            .get_matching_core(&instrument_id)
            .is_some_and(|core| core.order_exists(child_id)),
        expected_status == OrderStatus::Emulated
    );
}

fn cache_oto_recovery_snapshot(
    ctx: &TestContext,
    instrument: &InstrumentAny,
    strategy_id: StrategyId,
    position_id: PositionId,
    snapshot_opened: bool,
) {
    let qty = Quantity::from("1.000");
    let price = Price::from("3000.00");
    let snapshot_order = OrderTestBuilder::new(OrderType::Market)
        .client_order_id(ClientOrderId::from("O-SNAPSHOT"))
        .strategy_id(strategy_id)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .quantity(qty)
        .tags(vec![Ustr::from(RECONCILIATION_ORDER_TAG)])
        .build();
    let mut snapshot_fill = OrderFilledTestBuilder::new(&snapshot_order, instrument)
        .position_id(position_id)
        .account_id(test_account_id())
        .trade_id(TradeId::from("T-SNAPSHOT"))
        .last_qty(qty)
        .last_px(price)
        .ts_event(3_000_000.into())
        .build();

    let OrderEventAny::Filled(fill) = &mut snapshot_fill else {
        unreachable!()
    };

    fill.reconciliation = true;
    let position = if snapshot_opened {
        Position::new(instrument, fill.clone())
    } else {
        let mut opening = fill.clone();
        opening.client_order_id = ClientOrderId::from("O-ORDINARY-REOPEN");
        opening.trade_id = TradeId::from("T-ORDINARY-REOPEN");
        opening.ts_event = 2_000_000.into();
        opening.event_id = UUID4::new();
        opening.reconciliation = false;
        let mut position = Position::new(instrument, opening);
        position.apply(fill);
        position
    };

    ctx.add_order(snapshot_order);
    ctx.cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();
}

#[tokio::test]
async fn test_fill_before_retained_netting_lifecycle_projects_order_only() {
    let config = ExecutionManagerConfig {
        generate_missing_orders: false,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let strategy_id = StrategyId::from("STRATEGY-001");
    let old_order_id = ClientOrderId::from("O-PRIOR-LIFECYCLE");
    let old_venue_order_id = VenueOrderId::from("V-PRIOR-LIFECYCLE");
    let old_trade_id = TradeId::from("T-PRIOR-LIFECYCLE");
    let current_order_id = ClientOrderId::from("O-CURRENT-LIFECYCLE");
    let current_venue_order_id = VenueOrderId::from("V-CURRENT-LIFECYCLE");
    let current_trade_id = TradeId::from("T-CURRENT-LIFECYCLE");
    let position_id = PositionId::new(format!("{instrument_id}-{strategy_id}"));

    ctx.add_instrument(instrument.clone());
    ctx.manager
        .claim_external_orders(instrument_id, strategy_id)
        .unwrap();
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(strategy_id, OmsType::Netting);

    let mut current_order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(current_order_id)
        .strategy_id(strategy_id)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.000"))
        .price(Price::from("3200.00"))
        .build();
    apply_submitted_and_accepted(&mut current_order, current_venue_order_id);
    let current_fill = TestOrderEventStubs::filled(
        &current_order,
        &instrument,
        Some(current_trade_id),
        Some(position_id),
        Some(Price::from("3200.00")),
        Some(Quantity::from("1.000")),
        Some(LiquiditySide::Maker),
        Some(Money::from("0.50 USDT")),
        Some(UnixNanos::from(3_000_000)),
        Some(test_account_id()),
    );
    let position = Position::new(&instrument, current_fill.into());
    ctx.cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let old_order_report = create_order_report(
        Some(old_order_id),
        old_venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("1.000"),
        Quantity::from("1.000"),
    )
    .with_avg_px(dec!(3000.0));
    let current_order_report = create_order_report(
        Some(current_order_id),
        current_venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("1.000"),
        Quantity::from("1.000"),
    )
    .with_avg_px(dec!(3200.0));

    let old_fill_report = FillReport::new(
        test_account_id(),
        instrument_id,
        old_venue_order_id,
        old_trade_id,
        OrderSide::Buy,
        Quantity::from("1.000"),
        Price::from("3000.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Maker,
        Some(old_order_id),
        None,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );

    let current_fill_report = FillReport::new(
        test_account_id(),
        instrument_id,
        current_venue_order_id,
        current_trade_id,
        OrderSide::Buy,
        Quantity::from("1.000"),
        Price::from("3200.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Maker,
        Some(current_order_id),
        None,
        UnixNanos::from(3_000_000),
        UnixNanos::from(3_000_000),
        None,
    );

    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("1.000"),
        UnixNanos::from(3_000_000),
        UnixNanos::from(3_000_000),
        None,
        None,
        Some(dec!(3200.00)),
    );

    mass_status.add_order_reports(vec![old_order_report, current_order_report]);
    mass_status.add_fill_reports(vec![old_fill_report, current_fill_report]);
    mass_status.add_position_reports(vec![position_report]);

    ctx.manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let cache = ctx.cache.borrow();
    let old_order = cache.order(&old_order_id).unwrap();
    let current_order = cache.order(&current_order_id).unwrap();
    let position = cache.position(&position_id).unwrap();

    assert_eq!(old_order.status(), OrderStatus::Filled);
    assert_eq!(current_order.status(), OrderStatus::Filled);
    assert_eq!(position.quantity, Quantity::from("1.000"));
    assert_eq!(position.realized_pnl, Some(Money::from("-0.50 USDT")));
    assert_eq!(position.commissions(), vec![Money::from("0.50 USDT")]);
    assert_eq!(position.trade_ids.len(), 1);
    assert!(position.trade_ids.contains(&current_trade_id));
    assert!(!position.trade_ids.contains(&old_trade_id));
}

#[tokio::test]
async fn test_adjust_fills_creates_synthetic_for_partial_window() {
    // Test that adjust_fills_for_partial_window creates synthetic fills when
    // historical fills don't fully explain the current position (partial window scenario).
    // This happens when lookback window started mid-position.
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

    // Position report shows LONG 5.0 with avg_px 3000.00
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.000"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None, // Netting mode
        Some(dec!(3000.00)),
    );

    mass_status.add_position_reports(vec![position_report]);

    // Only have fills for 2.0 (partial window - missing opening fills)
    let venue_order_id = VenueOrderId::from("V-PARTIAL-001");
    let order_report = create_order_report(
        Some(ClientOrderId::from("O-PARTIAL-001")),
        venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("2.000"),
        Quantity::from("2.000"),
    )
    .with_avg_px(dec!(3100.00));
    mass_status.add_order_reports(vec![order_report]);

    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-PARTIAL-001"),
        OrderSide::Buy,
        Quantity::from("2.000"),
        Price::from("3100.00"),
        Money::from("1.00 USDT"),
        LiquiditySide::Taker,
        None,
        None,
        UnixNanos::from(1_000_001),
        UnixNanos::from(1_000_001),
        None,
    );

    mass_status.add_fill_reports(vec![fill]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // The adjustment should create a synthetic fill for the missing 3.0
    // (position=5.0, fills=2.0, so synthetic opening of 3.0 is needed)
    // Events: Synthetic order (Accepted + Filled) + Original order (Accepted + Filled) + Position events
    // The exact number depends on implementation but we should have more than just the original fill

    // Verify we have fills that sum to match the position
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

    // Should have at least 2 fills (synthetic opening + original)
    assert!(
        fill_events.len() >= 2,
        "Expected at least 2 fills (synthetic + original), was {}",
        fill_events.len()
    );

    // Total filled quantity should match position quantity (5.0)
    let total_qty: f64 = fill_events.iter().map(|f| f.last_qty.as_f64()).sum();
    assert!(
        (total_qty - 5.0).abs() < 0.001,
        "Total filled qty should be ~5.0 to match position, was {total_qty}"
    );
}

#[rstest]
#[case::unclaimed(None)]
#[case::claimed(Some(StrategyId::from("CLAIMER-001")))]
#[tokio::test]
async fn test_replayed_fill_does_not_reopen_reconciled_position(
    #[case] claimed_strategy: Option<StrategyId>,
) {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let strategy_id = claimed_strategy.unwrap_or_else(StrategyId::external);
    let replay_venue_order_id = VenueOrderId::from("V-REPLAY-001");
    let replay_trade_id = TradeId::from("T-REPLAY-001");
    ctx.add_instrument(test_instrument());
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(strategy_id, OmsType::Netting);

    if claimed_strategy.is_some() {
        ctx.manager
            .claim_external_orders(instrument_id, strategy_id)
            .unwrap();
    }

    ctx.advance_time(10_000_000);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    mass_status.add_position_reports(vec![PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3000.50)),
    )]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);
    let synthetic_client_order_id = result.events[0].client_order_id();

    // The venue resends the execution the position report already covers
    let replayed = FillReport::new(
        test_account_id(),
        instrument_id,
        replay_venue_order_id,
        replay_trade_id,
        OrderSide::Buy,
        Quantity::from("5.0"),
        Price::from("3000.50"),
        Money::from("0.50 USDT"),
        LiquiditySide::Taker,
        None,
        None,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );

    ctx.exec_engine
        .borrow_mut()
        .reconcile_fill_report(&replayed);

    let synthetic_order = ctx.get_order(&synthetic_client_order_id).unwrap();
    let cache = ctx.cache.borrow();
    let replay_client_order_id = cache
        .client_order_id(&replay_venue_order_id)
        .copied()
        .unwrap();
    let replay_order = cache.order(&replay_client_order_id).unwrap();
    let positions = cache.positions_open(
        None,
        Some(&instrument_id),
        None,
        Some(&test_account_id()),
        None,
    );

    assert_eq!(synthetic_order.strategy_id(), strategy_id);
    assert_eq!(
        synthetic_order.tags(),
        Some(&[ustr::Ustr::from("RECONCILIATION")][..])
    );
    assert_eq!(replay_order.status(), OrderStatus::Filled);
    assert_eq!(replay_order.filled_qty(), Quantity::from("5.0"));
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].opening_order_id, synthetic_client_order_id);
    assert_eq!(positions[0].signed_decimal_qty(), dec!(5.0));
    assert!(!positions[0].trade_ids.contains(&replay_trade_id));
}

#[tokio::test]
async fn test_partial_window_adjustment_skips_hedge_mode_instruments() {
    // Partial-window fill adjustment should skip hedge mode instruments
    // (those with venue_position_id set) to avoid corrupting fills
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let instrument = test_instrument();
    ctx.add_instrument(instrument);

    let venue_order_id = VenueOrderId::from("V-HEDGE-001");

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Add a filled order report with fills
    let report = create_order_report(
        None,
        venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("5.0"),
        Quantity::from("5.0"),
    );
    mass_status.add_order_reports(vec![report]);

    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-HEDGE-001"),
        OrderSide::Buy,
        Quantity::from("5.0"),
        Price::from("100.0"),
        Money::from("0.10 USDT"),
        LiquiditySide::Taker,
        None,
        None,
        UnixNanos::from(1_000_001),
        UnixNanos::from(1_000_001),
        None,
    );

    mass_status.add_fill_reports(vec![fill]);

    // Add hedge mode position report (has venue_position_id)
    let hedge_position_id = PositionId::new("HEDGE-POS-001");

    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
        None,                    // report_id
        Some(hedge_position_id), // Hedge mode - has venue_position_id
        Some(dec!(100.0)),       // avg_px_open
    );

    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // The fill should be preserved (not modified by partial-window adjustment)
    // and external order should be created
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

    assert!(
        !fill_events.is_empty(),
        "Fill events should be preserved for hedge mode instruments"
    );

    // Verify the fill quantity matches original (wasn't modified)
    assert_eq!(
        fill_events[0].last_qty,
        Quantity::from("5.0"),
        "Fill quantity should match original fill report"
    );
}

#[tokio::test]
async fn test_adjust_fills_multi_instrument_preserves_all_fills() {
    // Test that adjusting fills for one instrument doesn't affect fills for another.
    let mut ctx = TestContext::new();
    let instrument_id1 = test_instrument_id();
    let instrument_id2 = test_instrument_id2();

    ctx.add_instrument(test_instrument());
    ctx.add_instrument(test_instrument2());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Instrument 1 (ETHUSDT) - position of 2.0, fills sum to 2.0 (complete history)
    let venue_order_id1a = VenueOrderId::from("V-ETH-001");
    let venue_order_id1b = VenueOrderId::from("V-ETH-002");

    let order_report1a = create_order_report(
        Some(ClientOrderId::from("O-ETH-001")),
        venue_order_id1a,
        instrument_id1,
        OrderStatus::Filled,
        Quantity::from("1.000"),
        Quantity::from("1.000"),
    );
    let order_report1b = create_order_report(
        Some(ClientOrderId::from("O-ETH-002")),
        venue_order_id1b,
        instrument_id1,
        OrderStatus::Filled,
        Quantity::from("1.000"),
        Quantity::from("1.000"),
    );

    let fill1a = FillReport::new(
        test_account_id(),
        instrument_id1,
        venue_order_id1a,
        TradeId::from("T-ETH-001"),
        OrderSide::Buy,
        Quantity::from("1.000"),
        Price::from("3000.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Taker,
        None,
        None,
        UnixNanos::from(1_000_001),
        UnixNanos::from(1_000_001),
        None,
    );

    let fill1b = FillReport::new(
        test_account_id(),
        instrument_id1,
        venue_order_id1b,
        TradeId::from("T-ETH-002"),
        OrderSide::Buy,
        Quantity::from("1.000"),
        Price::from("3100.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Taker,
        None,
        None,
        UnixNanos::from(1_000_002),
        UnixNanos::from(1_000_002),
        None,
    );

    let position_report1 = PositionStatusReport::new(
        test_account_id(),
        instrument_id1,
        PositionSide::Long,
        Quantity::from("2.000"),
        UnixNanos::from(2_000),
        UnixNanos::from(2_000),
        None,
        None,
        Some(dec!(3050.00)),
    );

    // Instrument 2 (BTCUSD) - position of 100, fills sum to 100 (complete history)
    let venue_order_id2a = VenueOrderId::from("V-BTC-001");
    let venue_order_id2b = VenueOrderId::from("V-BTC-002");

    let order_report2a = create_order_report(
        Some(ClientOrderId::from("O-BTC-001")),
        venue_order_id2a,
        instrument_id2,
        OrderStatus::Filled,
        Quantity::from("50"),
        Quantity::from("50"),
    );
    let order_report2b = create_order_report(
        Some(ClientOrderId::from("O-BTC-002")),
        venue_order_id2b,
        instrument_id2,
        OrderStatus::Filled,
        Quantity::from("50"),
        Quantity::from("50"),
    );

    let fill2a = FillReport::new(
        test_account_id(),
        instrument_id2,
        venue_order_id2a,
        TradeId::from("T-BTC-001"),
        OrderSide::Buy,
        Quantity::from("50"),
        Price::from("50000.0"),
        Money::from("0.001 BTC"),
        LiquiditySide::Taker,
        None,
        None,
        UnixNanos::from(1_000_003),
        UnixNanos::from(1_000_003),
        None,
    );

    let fill2b = FillReport::new(
        test_account_id(),
        instrument_id2,
        venue_order_id2b,
        TradeId::from("T-BTC-002"),
        OrderSide::Buy,
        Quantity::from("50"),
        Price::from("51000.0"),
        Money::from("0.001 BTC"),
        LiquiditySide::Taker,
        None,
        None,
        UnixNanos::from(1_000_004),
        UnixNanos::from(1_000_004),
        None,
    );

    let position_report2 = PositionStatusReport::new(
        test_account_id(),
        instrument_id2,
        PositionSide::Long,
        Quantity::from("100"),
        UnixNanos::from(2_500),
        UnixNanos::from(2_500),
        None,
        None,
        Some(dec!(50500.0)),
    );

    mass_status.add_order_reports(vec![
        order_report1a,
        order_report1b,
        order_report2a,
        order_report2b,
    ]);
    mass_status.add_fill_reports(vec![fill1a, fill1b, fill2a, fill2b]);
    mass_status.add_position_reports(vec![position_report1, position_report2]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

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
        4,
        "Expected 4 fill events (2 per instrument)"
    );

    let eth_fills: Vec<_> = fill_events
        .iter()
        .filter(|f| f.instrument_id == instrument_id1)
        .collect();
    assert_eq!(eth_fills.len(), 2, "Expected 2 fills for ETHUSDT");
    let eth_total_qty: f64 = eth_fills.iter().map(|f| f.last_qty.as_f64()).sum();
    assert!(
        (eth_total_qty - 2.0).abs() < 0.001,
        "ETHUSDT total qty should be 2.0, was {eth_total_qty}"
    );

    let btc_fills: Vec<_> = fill_events
        .iter()
        .filter(|f| f.instrument_id == instrument_id2)
        .collect();
    assert_eq!(btc_fills.len(), 2, "Expected 2 fills for BTCUSD");
    let btc_total_qty: f64 = btc_fills.iter().map(|f| f.last_qty.as_f64()).sum();
    assert!(
        (btc_total_qty - 100.0).abs() < 0.001,
        "BTCUSD total qty should be 100.0, was {btc_total_qty}"
    );
}

#[tokio::test]
async fn test_adjust_fills_missing_order_reports_uses_fill_side() {
    // Test that fills without order reports still use fill.order_side correctly
    // for partial-window adjustment calculations.
    //
    // Scenario: When position qty > fills qty, partial-window adjustment calculates
    // the net effect of fills and creates a synthetic fill to match the position.
    // The fill.order_side from fills (even without order reports) is used to
    // determine the direction of the synthetic fill.
    //
    // Note: Fills without order reports contribute to calculations but don't
    // directly produce events - only the synthetic fill does.
    let instrument_id = test_instrument_id();
    let instrument = test_instrument();

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Fill without order report: 0.02 BUY
    let venue_order_id1 = VenueOrderId::from("V-NO-REPORT-001");

    let fill1 = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id1,
        TradeId::from("T-001"),
        OrderSide::Buy, // This is the key: fill.order_side is BUY
        Quantity::from("0.020"),
        Price::from("4000.00"),
        Money::from("0.00 USDT"),
        LiquiditySide::Taker,
        None,
        None,
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
        None,
    );

    // Position report: 0.05 LONG (larger than our fill)
    // Synthetic fill of 0.03 BUY should be created to bridge the gap
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("0.050"),
        UnixNanos::from(2_000),
        UnixNanos::from(2_000),
        None,
        None,
        Some(dec!(3900.00)),
    );

    // No order reports - only fill1 with its fill.order_side
    mass_status.add_fill_reports(vec![fill1]);
    mass_status.add_position_reports(vec![position_report]);

    let result = process_mass_status_for_reconciliation(&mass_status, &instrument, None).unwrap();

    assert!(
        !result.orders.is_empty(),
        "Synthetic order should be created"
    );
    assert!(!result.fills.is_empty(), "Fills should be present");

    // Synthetic order direction should be inferred from fill.order_side
    for order in result.orders.values() {
        assert_eq!(
            order.order_side,
            OrderSide::Buy.into(),
            "Synthetic order side should be BUY (inferred from fill.order_side)"
        );
    }
}

#[tokio::test]
async fn test_adjust_fills_without_synthetic_reports_filters_to_current_lifecycle() {
    // Test FilterToCurrentLifecycle filters closed orders from previous lifecycles
    // while preserving working orders.
    //
    // Scenario:
    // - Position lifecycle: +100 (O1 BUY) -> FLAT (O2 SELL) -> +200 (O3 BUY current)
    // - O1 and O2 are FILLED (previous lifecycle, before zero-crossing)
    // - O3 is PARTIALLY_FILLED (working order in current lifecycle)
    // - Assert: O1 and O2 filtered out, O3 preserved
    let instrument_id = test_instrument_id();
    let instrument = test_instrument();
    let ts_now: u64 = 1_000_000_000_000;

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("200"),
        UnixNanos::from(ts_now),
        UnixNanos::from(ts_now),
        None,
        None,
        Some(dec!(1.1000)),
    );

    // O1: BUY 100 (previous lifecycle)
    let venue_order_id1 = VenueOrderId::from("V-001");
    let order_o1 = create_order_report(
        Some(ClientOrderId::from("C-001")),
        venue_order_id1,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("100"),
        Quantity::from("100"),
    );

    let fill_o1 = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id1,
        TradeId::from("T-001"),
        OrderSide::Buy,
        Quantity::from("100"),
        Price::from("1.0900"),
        Money::from("0.00 USDT"),
        LiquiditySide::Taker,
        None,
        None,
        UnixNanos::from(ts_now - 3_000_000_000),
        UnixNanos::from(ts_now - 3_000_000_000),
        None,
    );

    // O2: SELL 100 (zero-crossing to FLAT)
    let venue_order_id2 = VenueOrderId::from("V-002");

    let order_o2 = OrderStatusReport::new(
        test_account_id(),
        instrument_id,
        Some(ClientOrderId::from("C-002")),
        venue_order_id2,
        OrderSide::Sell.into(),
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Filled,
        Quantity::from("100"),
        Quantity::from("100"),
        UnixNanos::from(ts_now - 2_000_000_000),
        UnixNanos::from(ts_now - 2_000_000_000),
        UnixNanos::from(ts_now - 2_000_000_000),
        None,
    );

    let fill_o2 = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id2,
        TradeId::from("T-002"),
        OrderSide::Sell,
        Quantity::from("100"),
        Price::from("1.0950"),
        Money::from("0.00 USDT"),
        LiquiditySide::Taker,
        None,
        None,
        UnixNanos::from(ts_now - 2_000_000_000), // Zero-crossing here
        UnixNanos::from(ts_now - 2_000_000_000),
        None,
    );

    // O3: BUY 200 (current lifecycle, PARTIALLY_FILLED working order)
    let venue_order_id3 = VenueOrderId::from("V-003");
    let order_o3 = create_order_report(
        Some(ClientOrderId::from("C-003")),
        venue_order_id3,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("300"),
        Quantity::from("200"),
    );

    let fill_o3 = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id3,
        TradeId::from("T-003"),
        OrderSide::Buy,
        Quantity::from("200"),
        Price::from("1.1000"),
        Money::from("0.00 USDT"),
        LiquiditySide::Taker,
        None,
        None,
        UnixNanos::from(ts_now),
        UnixNanos::from(ts_now),
        None,
    );

    mass_status.add_order_reports(vec![order_o1, order_o2, order_o3]);
    mass_status.add_fill_reports(vec![fill_o1, fill_o2, fill_o3]);
    mass_status.add_position_reports(vec![position_report]);

    let result = process_mass_status_for_reconciliation_without_synthetic_reports(
        &mass_status,
        &instrument,
        None,
    )
    .unwrap();

    // O1 and O2 should be filtered out (closed orders from previous lifecycle)
    assert!(
        !result.orders.contains_key(&venue_order_id1),
        "O1 should be filtered (closed order from previous lifecycle)"
    );
    assert!(
        !result.orders.contains_key(&venue_order_id2),
        "O2 should be filtered (closed order from previous lifecycle)"
    );

    // O3 should be preserved (working order in current lifecycle)
    assert!(
        result.orders.contains_key(&venue_order_id3),
        "O3 should be preserved (working order)"
    );
    assert_eq!(
        result.orders[&venue_order_id3].order_status,
        OrderStatus::PartiallyFilled
    );

    // Only O3 fill should be present
    assert!(
        result.fills.contains_key(&venue_order_id3),
        "O3 fills should be present"
    );
    assert_eq!(result.fills.len(), 1, "Only O3 fills should remain");
}

#[rstest]
#[tokio::test]
async fn test_replace_current_lifecycle_preserves_working_orders(
    #[values(false, true)] bounded: bool,
    #[values(false, true)] terminal_report: bool,
) {
    let mut ctx = TestContext::new();
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(StrategyId::external(), OmsType::Netting);
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());
    let ts_now: u64 = 1_000_000_000_000;

    let make_fill = |venue_order_id: &str, trade_id: &str, side: OrderSide, px: &str, ts: u64| {
        FillReport::new(
            test_account_id(),
            instrument_id,
            VenueOrderId::from(venue_order_id),
            TradeId::from(trade_id),
            side,
            Quantity::from("1.000"),
            Price::from(px),
            Money::from("0.00 USDT"),
            LiquiditySide::Taker,
            None,
            None,
            UnixNanos::from(ts),
            UnixNanos::from(ts),
            None,
        )
    };

    let mut mass_status = create_mass_status(
        vec![
            create_order_report(
                Some(ClientOrderId::from("C-004")),
                VenueOrderId::from("V-004"),
                instrument_id,
                OrderStatus::PartiallyFilled,
                Quantity::from("2.000"),
                Quantity::from("1.000"),
            ),
            create_order_report(
                Some(ClientOrderId::from("C-009")),
                VenueOrderId::from("V-009"),
                instrument_id,
                OrderStatus::Accepted,
                Quantity::from("1.000"),
                Quantity::from("0.000"),
            ),
        ],
        vec![
            make_fill(
                "V-001",
                "T-001",
                OrderSide::Buy,
                "3000.00",
                ts_now - 4_000_000_000,
            ),
            make_fill(
                "V-002",
                "T-002",
                OrderSide::Sell,
                "3050.00",
                ts_now - 3_000_000_000,
            ),
            make_fill(
                "V-003",
                "T-003",
                OrderSide::Buy,
                "3000.00",
                ts_now - 2_000_000_000,
            ),
            make_fill(
                "V-004",
                "T-004",
                OrderSide::Buy,
                "3100.00",
                ts_now - 1_000_000_000,
            ),
        ],
    );

    if terminal_report {
        mass_status.add_order_reports(vec![create_order_report(
            Some(ClientOrderId::from("C-003")),
            VenueOrderId::from("V-003"),
            instrument_id,
            OrderStatus::Filled,
            Quantity::from("1.000"),
            Quantity::from("1.000"),
        )]);
    }

    mass_status.add_position_reports(vec![PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("1.000"),
        UnixNanos::from(ts_now),
        UnixNanos::from(ts_now),
        None,
        None,
        Some(dec!(3142.04)),
    )]);

    if bounded {
        mass_status.set_report_window(Some(UnixNanos::from(ts_now - 5_000_000_000)), true);
    }

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let accepted: Vec<ClientOrderId> = result
        .events
        .iter()
        .filter_map(|e| match e {
            OrderEventAny::Accepted(accepted) => Some(accepted.client_order_id),
            _ => None,
        })
        .collect();

    assert!(
        accepted.contains(&ClientOrderId::from("C-009")),
        "working order not adopted, events: {:?}",
        result.events
    );
    assert!(
        accepted.contains(&ClientOrderId::from("C-004")),
        "partially filled working order not adopted, events: {:?}",
        result.events
    );

    let fills: Vec<&OrderFilled> = result
        .events
        .iter()
        .filter_map(|e| match e {
            OrderEventAny::Filled(fill) => Some(fill),
            _ => None,
        })
        .collect();

    assert_eq!(
        fills.len(),
        2 + usize::from(terminal_report),
        "fills: {fills:?}"
    );
    let synthetic = fills
        .iter()
        .find(|fill| fill.trade_id.as_str().starts_with("S-"))
        .unwrap();
    assert!(synthetic.venue_order_id.as_str().starts_with("S-"));
    assert_eq!(synthetic.last_qty, Quantity::from("1.000"));
    assert!(result.unresolved_positions.is_empty());
    let cache = ctx.cache.borrow();
    if terminal_report {
        let terminal = cache.order(&ClientOrderId::from("C-003")).unwrap();
        assert_eq!(terminal.status(), OrderStatus::Filled);
        assert_eq!(terminal.filled_qty(), Quantity::from("1.000"));
        assert_eq!(terminal.trade_ids(), vec![&TradeId::from("T-003")]);
    }

    let order = cache.order(&ClientOrderId::from("C-004")).unwrap();
    assert_eq!(order.venue_order_id(), Some(VenueOrderId::from("V-004")));
    assert_eq!(order.status(), OrderStatus::PartiallyFilled);
    assert_eq!(order.quantity(), Quantity::from("2.000"));
    assert_eq!(order.filled_qty(), Quantity::from("1.000"));
    assert_eq!(order.leaves_qty(), Quantity::from("1.000"));
    assert_eq!(order.trade_ids(), vec![&TradeId::from("T-004")]);
    assert_eq!(cache.orders_open(None, None, None, None, None).len(), 2);
    let positions = cache.positions_open(None, Some(&instrument_id), None, None, None);
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].signed_decimal_qty(), dec!(1));
    assert_eq!(positions[0].avg_px_open, dec!(3142.04));
    drop(positions);
    drop(order);
    drop(cache);

    let replayed = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);
    assert!(
        replayed.unresolved_positions.is_empty(),
        "{:?}",
        replayed.unresolved_positions
    );
    let cache = ctx.cache.borrow();
    let positions = cache.positions_open(None, Some(&instrument_id), None, None, None);
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].signed_decimal_qty(), dec!(1));
    assert_eq!(positions[0].avg_px_open, dec!(3142.04));
    drop(positions);
    drop(cache);

    let order = ctx.get_order(&ClientOrderId::from("C-004")).unwrap();
    let fill: OrderFilled = TestOrderEventStubs::filled(
        &order,
        &test_instrument(),
        Some(TradeId::from("T-005")),
        None,
        Some(Price::from("3200.00")),
        Some(Quantity::from("1.000")),
        None,
        None,
        Some(UnixNanos::from(ts_now + 1)),
        Some(test_account_id()),
    )
    .into();
    ctx.exec_engine
        .borrow_mut()
        .process(&OrderEventAny::Filled(fill));

    let order = ctx.get_order(&ClientOrderId::from("C-004")).unwrap();
    assert_eq!(order.status(), OrderStatus::Filled);
    assert_eq!(order.filled_qty(), Quantity::from("2.000"));
    let cache = ctx.cache.borrow();
    let positions = cache.positions_open(None, Some(&instrument_id), None, None, None);
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].signed_decimal_qty(), dec!(2));
    assert_eq!(positions[0].avg_px_open, dec!(3171.02));
    assert_eq!(positions[0].trade_ids.len(), 2);
    assert!(positions[0].trade_ids.contains(&TradeId::from("T-005")));
    assert!(!positions[0].trade_ids.contains(&TradeId::from("T-004")));
}

#[rstest]
#[case::ordinary(false, false)]
#[case::duplicate(true, false)]
#[case::flat_crossing(false, true)]
#[tokio::test]
async fn test_fresh_cache_replacement_chain_preserves_native_fills(
    #[values(false, true)] child_fill: bool,
    #[values(false, true)] reverse_reports: bool,
    #[values(false, true)] parent_label: bool,
    #[case] duplicate_fill: bool,
    #[case] crosses_flat: bool,
) {
    let _log_guard = MANAGER_LOG_TEST_LOCK.lock().await;
    install_manager_log_capture();
    let mut ctx = TestContext::new();
    ctx.add_instrument(test_instrument());
    let strategy_id = StrategyId::from("STRATEGY-001");
    ctx.manager
        .claim_external_orders(test_instrument_id(), strategy_id)
        .unwrap();
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(strategy_id, OmsType::Netting);
    let mut client = MockExecutionClient::new(Vec::new());
    client.order_venue_ids.insert(
        ClientOrderId::from("O-FRESH-CHAIN"),
        vec![
            VenueOrderId::from("V-FRESH-ROOT"),
            VenueOrderId::from("V-FRESH-MIDDLE"),
            VenueOrderId::from("V-FRESH-CURRENT"),
        ],
    );

    {
        let mut engine = ctx.exec_engine.borrow_mut();
        engine.deregister_client(test_client_id()).unwrap();
        engine.register_client(Box::new(client)).unwrap();
    }

    let cid = ClientOrderId::from("O-FRESH-CHAIN");
    let root_id = VenueOrderId::from("V-FRESH-ROOT");
    let middle_id = VenueOrderId::from("V-FRESH-MIDDLE");
    let current_id = VenueOrderId::from("V-FRESH-CURRENT");

    let filled = if child_fill { "0.5" } else { "0.3" };
    let average = if child_fill {
        dec!(3500)
    } else {
        dec!(10300) / dec!(3)
    };

    let mut reports = vec![
        create_order_report(
            Some(cid),
            root_id,
            test_instrument_id(),
            OrderStatus::Canceled,
            Quantity::from("1.0"),
            Quantity::from("0.2"),
        ),
        create_order_report(
            Some(cid),
            middle_id,
            test_instrument_id(),
            OrderStatus::Canceled,
            Quantity::from("0.8"),
            Quantity::from("0.1"),
        ),
        create_order_report(
            Some(cid),
            current_id,
            test_instrument_id(),
            OrderStatus::PartiallyFilled,
            Quantity::from("1.0"),
            Quantity::from(filled),
        )
        .with_avg_px(average),
    ];

    for (index, report) in reports.iter_mut().enumerate() {
        report.ts_accepted = UnixNanos::from(100 + index as u64 * 50);
        report.ts_last = UnixNanos::from(900 + index as u64 * 50);
        report.price = Some(Price::from("3605.00"));
    }

    reports[2].ts_accepted = UnixNanos::from(100);
    if reverse_reports {
        reports.reverse();
    }

    let root_fill = FillReport::new(
        test_account_id(),
        test_instrument_id(),
        root_id,
        TradeId::from("T-FRESH-ROOT"),
        OrderSide::Buy,
        Quantity::from("0.2"),
        Price::from("3400.00"),
        Money::from("0.02 USDT"),
        LiquiditySide::Maker,
        parent_label.then_some(cid),
        None,
        UnixNanos::from(200),
        UnixNanos::from(2000),
        None,
    );

    let middle_fill = FillReport::new(
        test_account_id(),
        test_instrument_id(),
        middle_id,
        TradeId::from("T-FRESH-MIDDLE"),
        OrderSide::Buy,
        Quantity::from("0.1"),
        Price::from("3500.00"),
        Money::from("0.03 USDT"),
        LiquiditySide::Taker,
        parent_label.then_some(cid),
        None,
        UnixNanos::from(250),
        UnixNanos::from(2000),
        None,
    );

    let mut fills = vec![middle_fill, root_fill.clone()];
    if child_fill {
        fills.push(FillReport::new(
            test_account_id(),
            test_instrument_id(),
            current_id,
            TradeId::from("T-FRESH-CURRENT"),
            OrderSide::Buy,
            Quantity::from("0.2"),
            Price::from("3600.00"),
            Money::from("0.05 USDT"),
            LiquiditySide::Taker,
            Some(cid),
            None,
            UnixNanos::from(300),
            UnixNanos::from(2000),
            None,
        ));
    }

    if duplicate_fill {
        fills.push(root_fill);
    }

    let mut mass = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::from(2000),
        None,
    );

    mass.add_order_reports(reports);
    mass.add_fill_reports(fills);

    if crosses_flat {
        let close_id = VenueOrderId::from("V-FRESH-CLOSE");
        mass.add_order_reports(vec![
            create_order_report_for_side(
                Some(ClientOrderId::from("O-FRESH-CLOSE")),
                close_id,
                test_instrument_id(),
                OrderSide::Sell,
                OrderStatus::Filled,
                Quantity::from("0.3"),
                Quantity::from("0.3"),
            )
            .with_avg_px(dec!(3550)),
        ]);
        mass.add_fill_reports(vec![FillReport::new(
            test_account_id(),
            test_instrument_id(),
            close_id,
            TradeId::from("T-FRESH-CLOSE"),
            OrderSide::Sell,
            Quantity::from("0.3"),
            Price::from("3550.00"),
            Money::from("0.07 USDT"),
            LiquiditySide::Taker,
            Some(ClientOrderId::from("O-FRESH-CLOSE")),
            None,
            UnixNanos::from(275),
            UnixNanos::from(2000),
            None,
        )]);
        mass.add_position_reports(vec![PositionStatusReport::new(
            test_account_id(),
            test_instrument_id(),
            if child_fill {
                PositionSide::Long
            } else {
                PositionSide::Flat
            },
            Quantity::from(if child_fill { "0.2" } else { "0.0" }),
            UnixNanos::from(1000),
            UnixNanos::from(2000),
            None,
            None,
            Some(dec!(3600)),
        )]);
    }

    let first = ctx
        .manager
        .reconcile_execution_mass_status(&mass, &ctx.exec_engine);
    let repeated = ctx
        .manager
        .reconcile_execution_mass_status(&mass, &ctx.exec_engine);
    let order = ctx.get_order(&cid).unwrap();

    let fills: Vec<_> = first
        .events
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) if fill.client_order_id == cid => Some(fill),
            _ => None,
        })
        .collect();

    let mut expected_trades = vec![
        TradeId::from("T-FRESH-ROOT"),
        TradeId::from("T-FRESH-MIDDLE"),
    ];

    if child_fill {
        expected_trades.push(TradeId::from("T-FRESH-CURRENT"));
    }

    assert_eq!(order.status(), OrderStatus::PartiallyFilled);
    assert_eq!(order.venue_order_id(), Some(current_id));
    assert_eq!(
        order.venue_order_ids(),
        vec![&root_id, &middle_id, &current_id]
    );
    assert_eq!(order.quantity(), Quantity::from("1.0"));
    assert_eq!(order.filled_qty(), Quantity::from(filled));
    assert_eq!(order.avg_px(), Some(average));
    assert_eq!(
        order.trade_ids(),
        expected_trades.iter().collect::<Vec<_>>()
    );
    assert_eq!(
        order.commissions().get(&Currency::USDT()),
        Some(&Money::from(if child_fill {
            "0.10 USDT"
        } else {
            "0.05 USDT"
        }))
    );
    assert_eq!(fills.len(), if child_fill { 3 } else { 2 });
    assert_eq!(fills[0].venue_order_id, root_id);
    assert_eq!(fills[0].trade_id, TradeId::from("T-FRESH-ROOT"));
    assert_eq!(fills[0].last_qty, Quantity::from("0.2"));
    assert_eq!(fills[0].last_px, Price::from("3400.00"));
    assert_eq!(fills[0].commission, Some(Money::from("0.02 USDT")));
    assert_eq!(fills[0].liquidity_side, LiquiditySide::Maker);
    assert_eq!(fills[0].ts_event, UnixNanos::from(200));
    assert_eq!(fills[1].venue_order_id, middle_id);
    assert_eq!(fills[1].trade_id, TradeId::from("T-FRESH-MIDDLE"));
    assert_eq!(fills[1].last_qty, Quantity::from("0.1"));
    assert_eq!(fills[1].last_px, Price::from("3500.00"));
    assert_eq!(fills[1].commission, Some(Money::from("0.03 USDT")));
    assert_eq!(fills[1].liquidity_side, LiquiditySide::Taker);
    assert_eq!(fills[1].ts_event, UnixNanos::from(250));

    if child_fill {
        assert_eq!(fills[2].venue_order_id, current_id);
        assert_eq!(fills[2].trade_id, TradeId::from("T-FRESH-CURRENT"));
        assert_eq!(fills[2].last_qty, Quantity::from("0.2"));
        assert_eq!(fills[2].last_px, Price::from("3600.00"));
        assert_eq!(fills[2].commission, Some(Money::from("0.05 USDT")));
        assert_eq!(fills[2].liquidity_side, LiquiditySide::Taker);
        assert_eq!(fills[2].ts_event, UnixNanos::from(300));
    }

    assert!(
        repeated
            .events
            .iter()
            .all(|event| crosses_flat && event.client_order_id() != cid)
    );
    assert!(!MANAGER_LOG_CAPTURE.messages.lock().iter().any(|message| {
        message.starts_with("Error indexing venue order ID")
            || message.starts_with("Cannot initialize venue order ID history")
    }));
    let cache = ctx.cache.borrow();
    assert_eq!(
        cache.orders(None, None, None, None, None).len(),
        1 + usize::from(crosses_flat)
    );
    assert_eq!(cache.venue_order_id(&cid), Some(&current_id));

    for id in [root_id, middle_id, current_id] {
        assert_eq!(cache.client_order_id(&id), Some(&cid));
    }

    let positions = cache.positions_open(None, Some(&test_instrument_id()), None, None, None);
    assert_eq!(positions.len(), usize::from(!crosses_flat || child_fill));

    if positions.is_empty() {
        return;
    }

    assert_eq!(
        positions[0].quantity,
        Quantity::from(if crosses_flat { "0.2" } else { filled })
    );
    assert_eq!(
        positions[0].trade_ids,
        if crosses_flat {
            vec![TradeId::from("T-FRESH-CURRENT")]
        } else {
            expected_trades
        }
        .into_iter()
        .collect()
    );
    assert_eq!(
        positions[0].commissions(),
        vec![Money::from(if child_fill && !crosses_flat {
            "0.10 USDT"
        } else {
            "0.05 USDT"
        })]
    );
}

#[rstest]
#[case::ordinary_client(false)]
#[case::venue_id_history_client(true)]
#[tokio::test]
async fn test_fresh_cache_venue_id_history_requires_client_proof(#[case] venue_id_history: bool) {
    let mut ctx = TestContext::new();
    ctx.add_instrument(test_instrument());
    let mut client = MockExecutionClient::new(Vec::new());
    if venue_id_history {
        client.order_venue_ids.insert(
            ClientOrderId::from("O-LABEL-PROOF"),
            vec![
                VenueOrderId::from("V-LABEL-PARENT"),
                VenueOrderId::from("V-LABEL-CURRENT"),
            ],
        );
    }

    {
        let mut engine = ctx.exec_engine.borrow_mut();
        engine.deregister_client(test_client_id()).unwrap();
        engine.register_client(Box::new(client)).unwrap();
    }

    let cid = ClientOrderId::from("O-LABEL-PROOF");
    let parent_id = VenueOrderId::from("V-LABEL-PARENT");
    let current_id = VenueOrderId::from("V-LABEL-CURRENT");
    let reports = vec![
        create_order_report(
            Some(cid),
            parent_id,
            test_instrument_id(),
            OrderStatus::Canceled,
            Quantity::from("1.0"),
            Quantity::from("0.0"),
        )
        .with_price(Price::from("3500.00")),
        create_order_report(
            Some(cid),
            current_id,
            test_instrument_id(),
            OrderStatus::Accepted,
            Quantity::from("1.0"),
            Quantity::from("0.0"),
        )
        .with_price(Price::from("3500.00")),
    ];

    let mut mass = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::from(2000),
        None,
    );

    mass.add_order_reports(reports);
    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass, &ctx.exec_engine);
    let order = ctx.get_order(&cid).unwrap();
    assert_eq!(
        order.status(),
        if venue_id_history {
            OrderStatus::Accepted
        } else {
            OrderStatus::Canceled
        }
    );
    assert_eq!(
        order.venue_order_id(),
        Some(if venue_id_history {
            current_id
        } else {
            parent_id
        })
    );
    assert_eq!(order.quantity(), Quantity::from("1.0"));
    assert_eq!(order.filled_qty(), Quantity::from("0.0"));
    assert_eq!(
        result
            .events
            .iter()
            .filter(|event| matches!(event, OrderEventAny::Updated(_)))
            .count(),
        usize::from(venue_id_history)
    );
    assert_eq!(
        ctx.cache
            .borrow()
            .orders(None, None, None, None, None)
            .len(),
        1
    );
}

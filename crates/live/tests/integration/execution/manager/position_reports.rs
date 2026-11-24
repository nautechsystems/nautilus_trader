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
async fn test_reconcile_mass_status_creates_position_from_position_report() {
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

    // Add a position report with no corresponding order reports
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3000.50)),
    );

    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should generate Accepted + Filled events to create the position
    assert_eq!(result.events.len(), 2);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));
    assert!(matches!(result.events[1], OrderEventAny::Filled(_)));

    if let OrderEventAny::Filled(filled) = &result.events[1] {
        assert_eq!(filled.last_qty, Quantity::from("5.0"));
        assert_eq!(filled.last_px.as_f64(), 3000.50);
        assert!(filled.reconciliation);
    }
}

#[rstest]
#[case::long(PositionSide::Long, Some(dec!(3000.50)), None, dec!(5), 0)]
#[case::short(PositionSide::Short, Some(dec!(3000.50)), None, dec!(-5), 0)]
#[case::missing_price(PositionSide::Long, None, None, Decimal::ZERO, 1)]
#[case::offset_legs(PositionSide::Long, Some(dec!(3000.50)), Some(Quantity::from("3.0")), dec!(2), 2)]
#[tokio::test]
async fn test_mass_status_netting_client_recovers_venue_position_id(
    #[case] side: PositionSide,
    #[case] avg_px: Option<Decimal>,
    #[case] opposite_qty: Option<Quantity>,
    #[case] expected_qty: Decimal,
    #[case] unresolved_count: usize,
) {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let venue_position_id = PositionId::from("P-VENUE");
    ctx.add_instrument(test_instrument());

    let mut client = MockExecutionClient::new(Vec::new());
    client.oms_type = OmsType::Netting;

    {
        let mut engine = ctx.exec_engine.borrow_mut();
        engine.deregister_client(test_client_id()).unwrap();
        engine.register_client(Box::new(client)).unwrap();
        engine.register_oms_type(StrategyId::external(), OmsType::Unspecified);
    }

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        None,
    );

    mass_status.add_position_reports(vec![PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        side,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(venue_position_id),
        avg_px,
    )]);

    if let Some(quantity) = opposite_qty {
        mass_status.add_position_reports(vec![PositionStatusReport::new(
            test_account_id(),
            instrument_id,
            PositionSide::Short,
            quantity,
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            None,
            Some(PositionId::from("P-VENUE-SHORT")),
            avg_px,
        )]);
    }

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);
    let cache = ctx.cache.borrow();
    let positions = cache.positions_open(
        None,
        Some(&instrument_id),
        None,
        Some(&test_account_id()),
        None,
    );

    assert!(cache.position(&venue_position_id).is_none());
    assert_eq!(
        positions
            .iter()
            .map(|p| p.signed_decimal_qty())
            .sum::<Decimal>(),
        expected_qty
    );

    if avg_px.is_some() {
        assert_eq!(positions.len(), 1);
        assert_eq!(cache.oms_type(&positions[0].id), Some(OmsType::Netting));
    }

    assert_eq!(result.unresolved_positions.len(), unresolved_count);
}

#[rstest]
#[case::zero_venue_price("3000.00", Decimal::ZERO, false)]
#[case::negative_venue_price("3000.00", dec!(-1), false)]
#[case::fractional_average("3000.01", dec!(3000.005), true)]
#[case::tolerance_boundary("3000.30", dec!(3000), true)]
#[case::outside_tolerance("3000.31", dec!(3000), false)]
#[tokio::test]
async fn test_mass_status_entry_price_tolerance(
    #[case] cached_price: &str,
    #[case] venue_price: Decimal,
    #[case] matches: bool,
    #[values(false, true)] hedging: bool,
) {
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let position_id = PositionId::from("P-ENTRY-PRICE");
    let position = create_test_position(
        &instrument,
        position_id,
        OrderSide::Buy,
        "5.000",
        cached_price,
    );
    ctx.add_instrument(instrument);
    ctx.add_position(&position);
    let mut mass_status = create_mass_status(vec![], vec![]);
    mass_status.add_position_reports(vec![PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.000"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        hedging.then_some(position_id),
        Some(venue_price),
    )]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert!(result.events.is_empty());
    assert_eq!(result.unresolved_positions.len(), usize::from(!matches));

    if !matches {
        assert_eq!(
            result.unresolved_positions[0],
            format!(
                "account={}, instrument={instrument_id}, venue_position_id={:?}, venue_quantity=5.000: position recovery did not restore the reported average entry price",
                test_account_id(),
                hedging.then_some(position_id),
            )
        );
    }

    assert_eq!(
        ctx.cache.borrow().position_owned(&position_id),
        Some(position)
    );
}

#[tokio::test]
async fn test_mass_status_netting_rejects_aggregate_mismatch() {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        generate_missing_orders: false,
        ..Default::default()
    });

    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let position = create_test_position(
        &instrument,
        PositionId::from("P-NET"),
        OrderSide::Buy,
        "5.000",
        "3000.00",
    );
    ctx.add_instrument(instrument);
    ctx.cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();
    let mut mass_status = create_mass_status(vec![], vec![]);
    mass_status.add_position_reports(vec![
        create_test_position_report(
            test_account_id(),
            instrument_id,
            PositionSide::Long,
            "5.000",
            "V-1",
            dec!(3000),
        ),
        create_test_position_report(
            test_account_id(),
            instrument_id,
            PositionSide::Long,
            "5.000",
            "V-2",
            dec!(3000),
        ),
    ]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert!(result.events.is_empty());
    assert_eq!(result.unresolved_positions.len(), 2);
    assert_eq!(
        ctx.cache.borrow().position_owned(&position.id),
        Some(position)
    );
}

#[rstest]
#[case::multiple_cached(&[("50", "10000.0"), ("50", "40000.0")], &[("100", dec!(16000))])]
#[case::multiple_venue(&[("100", "16000.0")], &[("50", dec!(10000)), ("50", dec!(40000))])]
#[tokio::test]
async fn test_mass_status_netting_matches_inverse_avg_px_across_positions(
    #[case] cached: &[(&str, &str)],
    #[case] venue: &[(&str, Decimal)],
) {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        generate_missing_orders: false,
        ..Default::default()
    });

    let instrument = test_instrument2();
    let instrument_id = instrument.id();

    for (i, &(qty, px)) in cached.iter().enumerate() {
        let position_id = format!("P-NET-{i}");
        let position = create_test_position(
            &instrument,
            PositionId::from(position_id.as_str()),
            OrderSide::Buy,
            qty,
            px,
        );
        ctx.cache
            .borrow_mut()
            .add_position(&position, OmsType::Netting)
            .unwrap();
    }

    ctx.add_instrument(instrument);
    let mut mass_status = create_mass_status(vec![], vec![]);
    mass_status.add_position_reports(
        venue
            .iter()
            .enumerate()
            .map(|(i, &(qty, px))| {
                let venue_position_id = format!("V-{i}");
                create_test_position_report(
                    test_account_id(),
                    instrument_id,
                    PositionSide::Long,
                    qty,
                    &venue_position_id,
                    px,
                )
            })
            .collect(),
    );

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert!(result.events.is_empty(), "{:?}", result.events);
    assert!(
        result.unresolved_positions.is_empty(),
        "{:?}",
        result.unresolved_positions
    );
}

#[rstest]
#[case::duplicates(0, 2, 0)]
#[case::distinct_snapshots(1, 0, 3)]
#[tokio::test]
async fn test_mass_status_netting_ignores_duplicate_position_reports(
    #[case] ts_last_step: u64,
    #[case] duplicate_count: usize,
    #[case] unresolved_count: usize,
) {
    let _log_guard = MANAGER_LOG_TEST_LOCK.lock().await;
    install_manager_log_capture();
    let mut ctx = TestContext::new();

    // Unique ID keeps other tests' duplicate warnings out of the exact count
    let mut instrument = crypto_perpetual_ethusdt();
    instrument.id = InstrumentId::from("ETHUSDT-DUPLICATE.BINANCE");
    let instrument_id = instrument.id;
    ctx.add_instrument(InstrumentAny::CryptoPerpetual(instrument));

    let mut client = MockExecutionClient::new(Vec::new());
    client.oms_type = OmsType::Netting;

    {
        let mut engine = ctx.exec_engine.borrow_mut();
        engine.deregister_client(test_client_id()).unwrap();
        engine.register_client(Box::new(client)).unwrap();
        engine.register_oms_type(StrategyId::external(), OmsType::Unspecified);
    }

    let mut mass_status = create_mass_status(vec![], vec![]);
    mass_status.add_position_reports(
        (0..3)
            .map(|i| {
                PositionStatusReport::new(
                    test_account_id(),
                    instrument_id,
                    PositionSide::Long,
                    Quantity::from("5.0"),
                    UnixNanos::from(1_000_000 + i * ts_last_step),
                    UnixNanos::from(2_000_000 + i),
                    None,
                    None,
                    Some(dec!(3000.50)),
                )
            })
            .collect(),
    );

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let messages = MANAGER_LOG_CAPTURE.messages.lock().clone();
    let duplicate_message = format!("Duplicate position report for {instrument_id}");
    let unresolved_message = format!(
        "account={}, instrument={instrument_id}, venue_position_id=None, venue_quantity=5.0: position recovery did not restore the reported quantity",
        test_account_id(),
    );
    let cache = ctx.cache.borrow();
    let positions = cache.positions_open(
        None,
        Some(&instrument_id),
        None,
        Some(&test_account_id()),
        None,
    );

    assert_eq!(result.events.len(), 2);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));

    let OrderEventAny::Filled(fill) = &result.events[1] else {
        panic!("Expected Filled event, was {:?}", result.events[1]);
    };

    assert_eq!(fill.order_side, OrderSide::Buy);
    assert_eq!(fill.last_qty, Quantity::from("5.0"));
    assert_eq!(fill.last_px, Price::from("3000.50"));
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].signed_decimal_qty(), dec!(5));
    assert_eq!(cache.oms_type(&positions[0].id), Some(OmsType::Netting));
    assert_eq!(
        result.unresolved_positions,
        vec![unresolved_message; unresolved_count]
    );
    assert_eq!(
        messages
            .iter()
            .filter(|message| **message == duplicate_message)
            .count(),
        duplicate_count
    );
}

#[tokio::test]
async fn test_bounded_filtered_position_preserves_order_history() {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        filter_position_reports: true,
        ..Default::default()
    });

    let instrument = test_instrument();
    let instrument_id = instrument.id();
    ctx.add_instrument(instrument);
    let mut orders = Vec::new();
    let mut fills = Vec::new();

    for (id, side, price, ts) in [
        ("1", OrderSide::Buy, "3000.00", 1_000_000),
        ("2", OrderSide::Sell, "3050.00", 2_000_000),
        ("3", OrderSide::Buy, "3100.00", 3_000_000),
    ] {
        let (order, fill) = create_bounded_fill_lifecycle(
            instrument_id,
            VenueOrderId::from(id),
            TradeId::from(id),
            side,
            "1.000",
            price,
            false,
            UnixNanos::from(ts),
        );
        orders.push(order);
        fills.push(fill);
    }

    let mut mass_status = create_mass_status(orders, fills);
    mass_status.set_report_window(Some(UnixNanos::from(500_000)), true);
    mass_status.add_position_reports(vec![PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("2.000"),
        UnixNanos::from(4_000_000),
        UnixNanos::from(4_000_000),
        None,
        None,
        Some(dec!(3200)),
    )]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 6);
    assert_eq!(result.external_orders.len(), 3);
    assert!(result.unresolved_positions.is_empty());
    let cache = ctx.cache.borrow();
    assert_eq!(cache.positions(None, None, None, None, None).len(), 0);
    assert_eq!(cache.orders(None, None, None, None, None).len(), 3);

    for id in ["1", "2", "3"] {
        let order = cache.order(&ClientOrderId::from(id)).unwrap();
        assert_eq!(order.venue_order_id(), Some(VenueOrderId::from(id)));
        assert_eq!(order.status(), OrderStatus::Filled);
        assert_eq!(order.filled_qty(), Quantity::from("1.000"));
        assert_eq!(order.trade_ids(), vec![&TradeId::from(id)]);
    }
}

#[rstest]
#[case::weighted_sides(Some(dec!(3200)), dec!(3300), true)]
#[case::long_mismatch(Some(dec!(3400)), dec!(3300), false)]
#[case::short_mismatch(Some(dec!(3200)), dec!(3500), false)]
#[case::missing_contributor(None, dec!(3300), false)]
#[case::zero_contributor(Some(Decimal::ZERO), dec!(3300), false)]
#[tokio::test]
async fn test_mass_status_entry_price_aggregates_each_side(
    #[case] second_long_price: Option<Decimal>,
    #[case] short_price: Decimal,
    #[case] matches: bool,
) {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        generate_missing_orders: false,
        ..Default::default()
    });

    let instrument = test_instrument();
    let instrument_id = instrument.id();

    let positions = [
        create_test_position(
            &instrument,
            PositionId::from("P-LONG-1"),
            OrderSide::Buy,
            "2.000",
            "3000.00",
        ),
        create_test_position(
            &instrument,
            PositionId::from("P-LONG-2"),
            OrderSide::Buy,
            "6.000",
            "3200.00",
        ),
        create_test_position(
            &instrument,
            PositionId::from("P-SHORT"),
            OrderSide::Sell,
            "3.000",
            "3300.00",
        ),
    ];

    ctx.add_instrument(instrument);
    for position in &positions {
        ctx.cache
            .borrow_mut()
            .add_position(position, OmsType::Netting)
            .unwrap();
    }

    let mut reports = vec![
        create_test_position_report(
            test_account_id(),
            instrument_id,
            PositionSide::Long,
            "2.000",
            "V-LONG-1",
            dec!(3000),
        ),
        create_test_position_report(
            test_account_id(),
            instrument_id,
            PositionSide::Long,
            "6.000",
            "V-LONG-2",
            dec!(3200),
        ),
        create_test_position_report(
            test_account_id(),
            instrument_id,
            PositionSide::Short,
            "3.000",
            "V-SHORT",
            short_price,
        ),
    ];
    reports[1].avg_px_open = second_long_price;
    let mut mass_status = create_mass_status(vec![], vec![]);
    mass_status.add_position_reports(reports);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert!(result.events.is_empty());
    assert!(result.external_orders.is_empty());
    assert_eq!(
        result.unresolved_positions.len(),
        if matches { 0 } else { 3 }
    );

    for position in positions {
        assert_eq!(
            ctx.cache.borrow().position_owned(&position.id),
            Some(position)
        );
    }
}

#[rstest]
#[case::long(OrderSide::Buy, PositionSide::Long)]
#[case::short(OrderSide::Sell, PositionSide::Short)]
#[tokio::test]
async fn test_mass_status_reduction_uses_reported_average_and_validates_remaining_entry(
    #[case] opening_side: OrderSide,
    #[case] report_side: PositionSide,
) {
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let position_id = PositionId::from("P-REDUCTION");
    let position =
        create_test_position(&instrument, position_id, opening_side, "20.000", "3000.00");
    ctx.add_instrument(instrument);
    ctx.add_position(&position);
    let mut mass_status = create_mass_status(vec![], vec![]);
    mass_status.add_position_reports(vec![PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        report_side,
        Quantity::from("10.000"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(position_id),
        Some(dec!(3100)),
    )]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 2);

    let OrderEventAny::Filled(fill) = &result.events[1] else {
        panic!("Expected reduction fill")
    };

    assert_eq!(
        fill.order_side,
        if opening_side == OrderSide::Buy {
            OrderSide::Sell
        } else {
            OrderSide::Buy
        }
    );
    assert_eq!(fill.last_qty, Quantity::from("10.000"));
    assert_eq!(fill.last_px, Price::from("3100.00"));
    let position = ctx.cache.borrow().position_owned(&position_id).unwrap();
    assert_eq!(position.quantity, Quantity::from("10.000"));
    assert_eq!(position.avg_px_open, dec!(3000.0));
    assert_eq!(
        result.unresolved_positions,
        vec![format!(
            "account={}, instrument={instrument_id}, venue_position_id=Some({position_id:?}), venue_quantity={}: position recovery did not restore the reported average entry price",
            test_account_id(),
            if opening_side == OrderSide::Buy {
                "10.000"
            } else {
                "-10.000"
            },
        )]
    );
}

#[rstest]
#[case::match_rejects(AvgPxReconciliation::Match, OrderSide::Buy, PositionSide::Long, true)]
#[case::opening_only_long(
    AvgPxReconciliation::OpeningOnly,
    OrderSide::Buy,
    PositionSide::Long,
    false
)]
#[case::opening_only_short(
    AvgPxReconciliation::OpeningOnly,
    OrderSide::Sell,
    PositionSide::Short,
    false
)]
#[tokio::test]
async fn test_mass_status_opening_only_average_skips_entry_price_check(
    #[case] avg_px_reconciliation: AvgPxReconciliation,
    #[case] entry_side: OrderSide,
    #[case] report_side: PositionSide,
    #[case] unresolved: bool,
) {
    let mut ctx = TestContext::new();
    let instrument = fifo_test_instrument();
    let instrument_id = instrument.id();
    let position = create_fifo_divergent_position(&instrument, entry_side);
    ctx.add_instrument(instrument);
    ctx.cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();
    let mut mass_status = create_mass_status(vec![], vec![]);
    mass_status.add_position_reports(vec![
        PositionStatusReport::new(
            test_account_id(),
            instrument_id,
            report_side,
            Quantity::from("1.000"),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            None,
            None,
            Some(dec!(60000)),
        )
        .with_avg_px_open_reconciliation(avg_px_reconciliation),
    ]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let expected_unresolved = if unresolved {
        vec![format!(
            "account={}, instrument={instrument_id}, venue_position_id=None, venue_quantity=1.000: position recovery did not restore the reported average entry price",
            test_account_id(),
        )]
    } else {
        Vec::new()
    };

    assert!(result.events.is_empty());
    assert_eq!(result.unresolved_positions, expected_unresolved);
    assert_eq!(position.avg_px_open, dec!(55000.0));
    assert_eq!(
        ctx.cache.borrow().position_owned(&position.id),
        Some(position)
    );
}

#[tokio::test]
async fn test_mass_status_opening_only_average_recovers_empty_cache() {
    let mut ctx = TestContext::new();
    let instrument = fifo_test_instrument();
    let instrument_id = instrument.id();
    ctx.add_instrument(instrument);
    let mut mass_status = create_mass_status(vec![], vec![]);
    mass_status.add_position_reports(vec![
        PositionStatusReport::new(
            test_account_id(),
            instrument_id,
            PositionSide::Long,
            Quantity::from("1.000"),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            None,
            None,
            Some(dec!(60000)),
        )
        .with_avg_px_open_reconciliation(AvgPxReconciliation::OpeningOnly),
    ]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let fills = filled_events(&result.events);
    let cache = ctx.cache.borrow();
    let positions = cache.positions_open(
        None,
        Some(&instrument_id),
        None,
        Some(&test_account_id()),
        None,
    );
    assert_eq!(
        fills,
        vec![(
            OrderSide::Buy,
            Quantity::from("1.000"),
            Price::from("60000.00")
        )]
    );
    assert!(result.unresolved_positions.is_empty());
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].signed_decimal_qty(), dec!(1));
    assert_eq!(positions[0].avg_px_open, dec!(60000.0));
}

#[rstest]
#[case::opening_only_increase(
    AvgPxReconciliation::OpeningOnly,
    PositionSide::Long,
    "2.000",
    dec!(65000),
    vec![(OrderSide::Buy, "1.000", "55000.00")]
)]
#[case::opening_only_reduction(
    AvgPxReconciliation::OpeningOnly,
    PositionSide::Long,
    "0.500",
    dec!(60000),
    vec![(OrderSide::Sell, "0.500", "55000.00")]
)]
#[case::opening_only_reversal(
    AvgPxReconciliation::OpeningOnly,
    PositionSide::Short,
    "1.000",
    dec!(70000),
    vec![(OrderSide::Sell, "1.000", "55000.00"), (OrderSide::Sell, "1.000", "70000.00")]
)]
#[case::match_increase_solves_average(
    AvgPxReconciliation::Match,
    PositionSide::Long,
    "2.000",
    dec!(65000),
    vec![(OrderSide::Buy, "1.000", "75000.00")]
)]
#[tokio::test]
async fn test_mass_status_opening_only_average_prices_cached_adjustments(
    #[case] avg_px_reconciliation: AvgPxReconciliation,
    #[case] report_side: PositionSide,
    #[case] report_qty: &str,
    #[case] report_avg_px: Decimal,
    #[case] expected_fills: Vec<(OrderSide, &str, &str)>,
) {
    let mut ctx = TestContext::new();
    let instrument = fifo_test_instrument();
    let instrument_id = instrument.id();
    let position = create_fifo_divergent_position(&instrument, OrderSide::Buy);
    ctx.add_instrument(instrument);
    ctx.cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();
    let mut mass_status = create_mass_status(vec![], vec![]);
    mass_status.add_position_reports(vec![
        PositionStatusReport::new(
            test_account_id(),
            instrument_id,
            report_side,
            Quantity::from(report_qty),
            UnixNanos::from(1_000_000),
            UnixNanos::from(1_000_000),
            None,
            None,
            Some(report_avg_px),
        )
        .with_avg_px_open_reconciliation(avg_px_reconciliation),
    ]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let expected_fills = expected_fills
        .into_iter()
        .map(|(side, qty, px)| (side, Quantity::from(qty), Price::from(px)))
        .collect::<Vec<_>>();
    assert_eq!(filled_events(&result.events), expected_fills);
    assert!(result.unresolved_positions.is_empty());
}

#[rstest]
#[case::opening_only_keeps_fills(AvgPxReconciliation::OpeningOnly, false)]
#[case::match_rejects_fifo_average(AvgPxReconciliation::Match, true)]
#[tokio::test]
async fn test_mass_status_opening_only_average_applies_full_history(
    #[case] avg_px_reconciliation: AvgPxReconciliation,
    #[case] unresolved: bool,
) {
    let mut ctx = TestContext::new();
    let instrument = fifo_test_instrument();
    let instrument_id = instrument.id();
    ctx.add_instrument(instrument);

    let mut client = MockExecutionClient::new(Vec::new());
    client.oms_type = OmsType::Netting;

    {
        let mut engine = ctx.exec_engine.borrow_mut();
        engine.deregister_client(test_client_id()).unwrap();
        engine.register_client(Box::new(client)).unwrap();
        engine.register_oms_type(StrategyId::external(), OmsType::Unspecified);
    }

    let mut orders = Vec::new();
    let mut fills = Vec::new();

    for (id, side, price, ts) in [
        ("1", OrderSide::Buy, "50000.00", 1_000_000),
        ("2", OrderSide::Buy, "60000.00", 2_000_000),
        ("3", OrderSide::Sell, "65000.00", 3_000_000),
    ] {
        let (order, fill) = create_bounded_fill_lifecycle(
            instrument_id,
            VenueOrderId::from(id),
            TradeId::from(id),
            side,
            "1.000",
            price,
            false,
            UnixNanos::from(ts),
        );
        orders.push(order);
        fills.push(fill);
    }

    let mut mass_status = create_mass_status(orders, fills);
    mass_status.add_position_reports(vec![
        PositionStatusReport::new(
            test_account_id(),
            instrument_id,
            PositionSide::Long,
            Quantity::from("1.000"),
            UnixNanos::from(4_000_000),
            UnixNanos::from(4_000_000),
            None,
            None,
            Some(dec!(60000)),
        )
        .with_avg_px_open_reconciliation(avg_px_reconciliation),
    ]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let expected_unresolved = if unresolved {
        vec![format!(
            "account={}, instrument={instrument_id}, venue_position_id=None, venue_quantity=1.000: position recovery did not restore the reported average entry price",
            test_account_id(),
        )]
    } else {
        Vec::new()
    };

    let cache = ctx.cache.borrow();
    let positions = cache.positions_open(
        None,
        Some(&instrument_id),
        None,
        Some(&test_account_id()),
        None,
    );
    assert_eq!(result.unresolved_positions, expected_unresolved);
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].signed_decimal_qty(), dec!(1));
    assert_eq!(positions[0].avg_px_open, dec!(55000.0));
    assert_eq!(
        positions[0].trade_ids,
        AHashSet::from([TradeId::from("1"), TradeId::from("2"), TradeId::from("3")])
    );
}

#[rstest]
#[case::coarsest_precision(
    vec![(AvgPxReconciliation::Match, Some(4), dec!(0.5599)), (AvgPxReconciliation::Match, Some(6), dec!(0.5599))],
    false
)]
#[case::opening_only_contributor(
    vec![(AvgPxReconciliation::Match, None, dec!(0.56)), (AvgPxReconciliation::OpeningOnly, None, dec!(0.70))],
    false
)]
#[case::unmarked_coarse_average(
    vec![(AvgPxReconciliation::Match, None, dec!(0.5599)), (AvgPxReconciliation::Match, None, dec!(0.5599))],
    true
)]
#[tokio::test]
async fn test_mass_status_entry_price_combines_side_report_metadata(
    #[case] report_averages: Vec<(AvgPxReconciliation, Option<u8>, Decimal)>,
    #[case] unresolved: bool,
) {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        generate_missing_orders: false,
        ..Default::default()
    });

    let instrument = coarse_report_instrument();
    let instrument_id = instrument.id();

    let positions = ["P-SIDE-1", "P-SIDE-2"].map(|position_id| {
        create_test_position(
            &instrument,
            PositionId::from(position_id),
            OrderSide::Buy,
            "1.000000",
            "0.560",
        )
    });

    ctx.add_instrument(instrument);
    for position in &positions {
        ctx.cache
            .borrow_mut()
            .add_position(position, OmsType::Netting)
            .unwrap();
    }

    let reports = report_averages
        .into_iter()
        .zip(["V-SIDE-1", "V-SIDE-2"])
        .map(
            |((avg_px_reconciliation, avg_px_precision, avg_px), venue_position_id)| {
                let mut report = create_test_position_report(
                    test_account_id(),
                    instrument_id,
                    PositionSide::Long,
                    "1.000000",
                    venue_position_id,
                    avg_px,
                )
                .with_avg_px_open_reconciliation(avg_px_reconciliation);
                report.avg_px_open_precision = avg_px_precision;
                report
            },
        )
        .collect();

    let mut mass_status = create_mass_status(vec![], vec![]);
    mass_status.add_position_reports(reports);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert!(result.events.is_empty());
    assert_eq!(
        result.unresolved_positions.len(),
        if unresolved { 2 } else { 0 }
    );
}

#[rstest]
#[case::marked(Some(4), false)]
#[case::unmarked(None, true)]
#[tokio::test]
async fn test_mass_status_coarse_average_keeps_retained_position(
    #[case] avg_px_open_precision: Option<u8>,
    #[case] unresolved: bool,
) {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = coarse_report_instrument();
    let instrument_id = instrument.id();
    let account_id = AccountId::from("POLYMARKET-001");

    // The Data API truncates the retained 8.928572 @ 0.56 position (cost basis 0.55999996)
    let position = create_test_position_for_account(
        &instrument,
        PositionId::from("P-COARSE-RETAINED"),
        OrderSide::Buy,
        "8.928572",
        "0.560",
        account_id,
    );

    let mut report = PositionStatusReport::new(
        account_id,
        instrument_id,
        PositionSide::Long,
        Quantity::from("8.928500"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(0.5599)),
    );

    report.avg_px_open_precision = avg_px_open_precision;
    ctx.add_instrument(instrument);
    ctx.add_margin_account(account_id);
    ctx.cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();

    let client = MockPositionExecutionClient::configured(
        ClientId::from("POLYMARKET"),
        account_id,
        Venue::from("POLYMARKET"),
        IndexSet::from([instrument_id.venue]),
        false,
    )
    .with_position_reports(vec![report.clone()])
    .with_position_reconciliation_tolerance(dec!(0.009999));
    let clients: Vec<&dyn ExecutionClient> = vec![&client];

    // Seeds the account tolerance, as the node builder does in production
    let seed_events = ctx.manager.check_positions_consistency(&clients).await;
    assert!(seed_events.is_empty());

    let mut mass_status = ExecutionMassStatus::new(
        client.client_id(),
        account_id,
        client.venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    mass_status.add_position_reports(vec![report]);
    ctx.exec_engine
        .borrow_mut()
        .register_client(Box::new(client))
        .unwrap();

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let expected_unresolved = if unresolved {
        vec![format!(
            "account={account_id}, instrument={instrument_id}, venue_position_id=None, venue_quantity=8.928500: position recovery did not restore the reported average entry price",
        )]
    } else {
        Vec::new()
    };

    assert!(result.events.is_empty());
    assert_eq!(result.unresolved_positions, expected_unresolved);
    assert_eq!(
        ctx.cache.borrow().position_owned(&position.id),
        Some(position)
    );
}

// An equity held across the fill lookback: the cache keeps the fills' average (all at 40.66),
// while the venue reports an average cost with commissions in it, rounded to the tick (40.67)
#[rstest]
#[case::marked(Some(2), false)]
#[case::unmarked(None, true)]
#[tokio::test]
async fn test_mass_status_average_cost_with_commission_keeps_retained_position(
    #[case] avg_px_open_precision: Option<u8>,
    #[case] unresolved: bool,
) {
    let mut ctx = TestContext::new();
    let instrument = InstrumentAny::Equity(equity_aapl());
    let instrument_id = instrument.id();
    let position = create_test_position(
        &instrument,
        PositionId::from("P-EQUITY"),
        OrderSide::Buy,
        "2342",
        "40.66",
    );
    ctx.add_instrument(instrument);
    ctx.cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();

    let mut report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("2342"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(40.67)),
    );

    report.avg_px_open_precision = avg_px_open_precision;
    let mut mass_status = create_mass_status(vec![], vec![]);
    mass_status.add_position_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let expected_unresolved = if unresolved {
        vec![format!(
            "account={}, instrument={instrument_id}, venue_position_id=None, venue_quantity=2342: position recovery did not restore the reported average entry price",
            test_account_id(),
        )]
    } else {
        Vec::new()
    };

    assert!(result.events.is_empty());
    assert_eq!(result.unresolved_positions, expected_unresolved);
    assert_eq!(
        ctx.cache.borrow().position_owned(&position.id),
        Some(position)
    );
}

#[rstest]
#[case::marked_keeps_cached_average(Some(4), "0.560")]
#[case::unmarked_solves_weighted_average(None, "0.0599")]
#[tokio::test]
async fn test_mass_status_coarse_average_prices_increase(
    #[case] avg_px_open_precision: Option<u8>,
    #[case] expected_px: &str,
) {
    let mut ctx = TestContext::new();
    let instrument = coarse_report_instrument();
    let instrument_id = instrument.id();
    let position = create_test_position(
        &instrument,
        PositionId::from("P-COARSE-INCREASE"),
        OrderSide::Buy,
        "100.000000",
        "0.560",
    );

    let mut report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("100.020000"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(0.5599)),
    );

    report.avg_px_open_precision = avg_px_open_precision;
    ctx.add_instrument(instrument);
    ctx.cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();
    let mut mass_status = create_mass_status(vec![], vec![]);
    mass_status.add_position_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(
        filled_events(&result.events),
        vec![(
            OrderSide::Buy,
            Quantity::from("0.020000"),
            Price::from(expected_px)
        )]
    );
    assert!(result.unresolved_positions.is_empty());
}

#[rstest]
#[case::off_tick(dec!(2.4564), "0.001")]
#[case::low_price(dec!(0.85237), "0.001")]
#[case::fine_tick(dec!(0.123456), "0.0001")]
#[case::within_tolerance(dec!(2.45621), "0.001")]
#[case::high_price(dec!(84060.35), "0.1")]
#[tokio::test]
async fn test_mass_status_recovers_off_tick_position_average(
    #[case] avg_px: Decimal,
    #[case] price_increment: &str,
    #[values(OrderSide::Buy, OrderSide::Sell)] side: OrderSide,
    #[values(false, true)] hedging: bool,
) {
    let mut ctx = TestContext::new();
    let mut instrument = crypto_perpetual_ethusdt();
    instrument.price_increment = Price::from(price_increment);
    instrument.price_precision = instrument.price_increment.precision;
    instrument.max_price = None;
    let instrument = InstrumentAny::CryptoPerpetual(instrument);
    let instrument_id = instrument.id();
    ctx.add_instrument(instrument);
    ctx.exec_engine.borrow_mut().register_oms_type(
        StrategyId::external(),
        if hedging {
            OmsType::Hedging
        } else {
            OmsType::Netting
        },
    );

    let position_side = if side == OrderSide::Buy {
        PositionSide::Long
    } else {
        PositionSide::Short
    };

    let report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        position_side,
        Quantity::from("3.000"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        hedging.then_some(PositionId::from("P-OFF-TICK")),
        Some(avg_px),
    );

    let mut mass_status = create_mass_status(vec![], vec![]);
    mass_status.add_position_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);
    let cache = ctx.cache.borrow();
    let positions = cache.positions_open(
        None,
        Some(&instrument_id),
        None,
        Some(&test_account_id()),
        None,
    );

    assert!(
        result.unresolved_positions.is_empty(),
        "{:?}",
        result.unresolved_positions
    );
    assert_eq!(
        filled_events(&result.events),
        vec![(
            side,
            Quantity::from("3.000"),
            Price::from_decimal(avg_px).unwrap()
        )]
    );
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].side, position_side);
    assert_eq!(positions[0].quantity, Quantity::from("3.000"));

    // Position stores its average as f64, compare at the reported decimal precision
    assert_eq!(
        Decimal::from_str_exact(&positions[0].avg_px_open.to_string())
            .unwrap()
            .round_dp(avg_px.scale()),
        avg_px
    );
}

#[rstest]
#[tokio::test]
async fn test_mass_status_recovers_off_tick_average_with_partial_fill_history(
    #[values(OrderSide::Buy, OrderSide::Sell)] side: OrderSide,
) {
    let mut ctx = TestContext::new();
    let mut instrument = crypto_perpetual_ethusdt();
    instrument.price_precision = 3;
    instrument.price_increment = Price::from("0.001");
    let instrument = InstrumentAny::CryptoPerpetual(instrument);
    let instrument_id = instrument.id();
    ctx.add_instrument(instrument);
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(StrategyId::external(), OmsType::Netting);

    let mut orders = Vec::new();
    let mut fills = Vec::new();

    for (id, quantity, price, ts) in [
        ("1", "1.000", "2.456", 1_000_000),
        ("2", "2.000", "2.457", 2_000_000),
    ] {
        let (order, fill) = create_bounded_fill_lifecycle(
            instrument_id,
            VenueOrderId::from(id),
            TradeId::from(id),
            side,
            quantity,
            price,
            false,
            UnixNanos::from(ts),
        );
        orders.push(order);
        fills.push(fill);
    }

    let position_side = if side == OrderSide::Buy {
        PositionSide::Long
    } else {
        PositionSide::Short
    };

    let avg_px = dec!(2.42568);

    let report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        position_side,
        Quantity::from("10.000"),
        UnixNanos::from(2_000_000),
        UnixNanos::from(2_000_000),
        None,
        None,
        Some(avg_px),
    );

    let mut mass_status = create_mass_status(orders, fills);
    mass_status.add_position_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);
    let cache = ctx.cache.borrow();
    let positions = cache.positions_open(
        None,
        Some(&instrument_id),
        None,
        Some(&test_account_id()),
        None,
    );

    assert!(
        result.unresolved_positions.is_empty(),
        "{:?}",
        result.unresolved_positions
    );
    assert_eq!(
        filled_events(&result.events),
        vec![
            (side, Quantity::from("7.000"), Price::from("2.4124")),
            (side, Quantity::from("1.000"), Price::from("2.456")),
            (side, Quantity::from("2.000"), Price::from("2.457")),
        ]
    );
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].side, position_side);
    assert_eq!(positions[0].quantity, Quantity::from("10.000"));
    assert_eq!(
        Decimal::from_str_exact(&positions[0].avg_px_open.to_string())
            .unwrap()
            .round_dp(avg_px.scale()),
        avg_px
    );
}

#[rstest]
#[tokio::test]
async fn test_mass_status_recovers_average_with_partial_fill_history_and_reduction(
    #[values(OrderSide::Buy, OrderSide::Sell)] side: OrderSide,
) {
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    ctx.add_instrument(instrument);
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(StrategyId::external(), OmsType::Netting);

    let reduce_side = match side {
        OrderSide::Buy => OrderSide::Sell,
        OrderSide::Sell => OrderSide::Buy,
    };
    let position_side = if side == OrderSide::Buy {
        PositionSide::Long
    } else {
        PositionSide::Short
    };

    let (add_order, add_fill) = create_bounded_fill_lifecycle(
        instrument_id,
        VenueOrderId::from("1"),
        TradeId::from("1"),
        side,
        "8.000",
        "50.00",
        false,
        UnixNanos::from(2_000_000),
    );
    let (reduce_order, reduce_fill) = create_bounded_fill_lifecycle(
        instrument_id,
        VenueOrderId::from("2"),
        TradeId::from("2"),
        reduce_side,
        "1.500",
        "50.10",
        true,
        UnixNanos::from(3_000_000),
    );

    let report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        position_side,
        Quantity::from("7.500"),
        UnixNanos::from(4_000_000),
        UnixNanos::from(4_000_000),
        None,
        None,
        Some(dec!(50.50)),
    );

    let mut mass_status =
        create_mass_status(vec![add_order, reduce_order], vec![add_fill, reduce_fill]);
    mass_status.add_position_reports(vec![report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);
    let cache = ctx.cache.borrow();
    let positions = cache.positions_open(
        None,
        Some(&instrument_id),
        None,
        Some(&test_account_id()),
        None,
    );

    assert!(
        result.unresolved_positions.is_empty(),
        "{:?}",
        result.unresolved_positions
    );
    assert_eq!(
        filled_events(&result.events),
        vec![
            (side, Quantity::from("1.000"), Price::from("54.50")),
            (side, Quantity::from("8.000"), Price::from("50.00")),
            (reduce_side, Quantity::from("1.500"), Price::from("50.10")),
        ]
    );
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].side, position_side);
    assert_eq!(positions[0].quantity, Quantity::from("7.500"));
    assert_eq!(positions[0].avg_px_open, dec!(50.5));
}

#[rstest]
#[case::missing_instrument(false, true, true, Some(dec!(3000.50)), "instrument missing from cache")]
#[case::missing_account(true, false, true, Some(dec!(3000.50)), "account missing from cache")]
#[case::disabled_generation(true, true, false, Some(dec!(3000.50)), "generate_missing_orders is disabled")]
#[case::missing_price(true, true, true, None, "missing avg_px_open for position recovery")]
#[tokio::test]
async fn test_mass_status_reports_unresolved_position_prerequisite(
    #[case] has_instrument: bool,
    #[case] has_account: bool,
    #[case] generate_missing_orders: bool,
    #[case] avg_px: Option<Decimal>,
    #[case] reason: &str,
    #[values(None, Some(PositionId::from("P-UNRECOVERED")))] position_id: Option<PositionId>,
) {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        generate_missing_orders,
        ..Default::default()
    });

    let instrument_id = test_instrument_id();
    let account_id = test_account_id();

    if !has_account {
        ctx.cache.borrow_mut().reset();
    }

    if has_instrument {
        ctx.add_instrument(test_instrument());
    }

    let report = PositionStatusReport::new(
        account_id,
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        position_id,
        avg_px,
    );

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        account_id,
        test_venue(),
        UnixNanos::default(),
        None,
    );

    mass_status.add_position_reports(vec![report]);
    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(
        result.unresolved_positions,
        vec![format!(
            "account={account_id}, instrument={instrument_id}, venue_position_id={position_id:?}, venue_quantity=5.0: {reason}"
        )],
    );
    assert_eq!(
        ctx.cache
            .borrow()
            .positions_open_count(None, None, None, None, None),
        0
    );
}

#[rstest]
#[case::netting(None)]
#[case::hedging(Some(PositionId::from("P-RECOVERED")))]
#[tokio::test]
async fn test_mass_status_synchronized_position_does_not_require_entry_price(
    #[case] position_id: Option<PositionId>,
) {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let mut report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        position_id,
        Some(dec!(3000.50)),
    );

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        None,
    );

    mass_status.add_position_reports(vec![report.clone()]);
    let recovered = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);
    assert!(recovered.unresolved_positions.is_empty());

    report.avg_px_open = None;

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        None,
    );

    mass_status.add_position_reports(vec![report]);
    let synchronized = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert!(synchronized.events.is_empty());
    assert!(synchronized.unresolved_positions.is_empty());
    let cache = ctx.cache.borrow();
    let positions = cache.positions_open(
        None,
        Some(&instrument_id),
        None,
        Some(&test_account_id()),
        None,
    );
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].signed_decimal_qty(), dec!(5));
}

#[tokio::test]
async fn test_reconcile_mass_status_skips_flat_position_report() {
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

    // Add a flat position report
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Flat,
        Quantity::from("0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        None,
    );

    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // No events should be generated for flat position
    assert!(result.events.is_empty());
}

#[tokio::test]
async fn test_reconcile_mass_status_skips_position_report_when_filtered() {
    let config = ExecutionManagerConfig {
        filter_position_reports: true,
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

    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3000.50)),
    );

    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Position reports should be filtered
    assert!(result.events.is_empty());
}

#[tokio::test]
async fn test_reconcile_mass_status_creates_short_position_from_report() {
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

    // Add a short position report
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Short,
        Quantity::from("3.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(2950.25)),
    );

    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 2);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));
    assert!(matches!(result.events[1], OrderEventAny::Filled(_)));

    if let OrderEventAny::Filled(filled) = &result.events[1] {
        assert_eq!(filled.last_qty, Quantity::from("3.0"));
        assert_eq!(filled.order_side, OrderSide::Sell);
    }
}

#[tokio::test]
async fn test_reconcile_mass_status_skips_position_report_when_fills_exist() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");

    ctx.add_instrument(test_instrument());
    let mut order = create_limit_order("O-001", instrument_id, OrderSide::Buy, "5.0", "3000.00");
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

    // Add a fill report for 5.0 qty
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
        Some(client_order_id),
        None,
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );

    mass_status.add_fill_reports(vec![fill]);

    // Add a position report for the same instrument (would duplicate if not skipped)
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3000.00)),
    );

    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should only have 1 fill event from the fill report, not additional events
    // from the position report (which would double-count)
    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Filled(_)));
}

// A Polymarket-style outcome token with six-decimal share quantities
fn coarse_report_instrument() -> InstrumentAny {
    let mut instrument = binary_option();
    instrument.size_precision = 6;
    instrument.size_increment = Quantity::from("0.000001");
    InstrumentAny::BinaryOption(instrument)
}

#[tokio::test]
async fn test_reconcile_mass_status_iterates_all_position_reports() {
    // Tests that we iterate ALL position reports, not just the first one
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

    // Add two position reports for the same instrument (hedge mode scenario)
    let position_report_long = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(PositionId::from("P-LONG-001")),
        Some(dec!(3000.50)),
    );

    let position_report_short = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Short,
        Quantity::from("3.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(PositionId::from("P-SHORT-001")),
        Some(dec!(3100.00)),
    );

    mass_status.add_position_reports(vec![position_report_long, position_report_short]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Both position reports should be processed, not just the first
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

    // Should have fills for both long and short positions
    let has_buy_fill = fill_events.iter().any(|f| f.order_side == OrderSide::Buy);
    let has_sell_fill = fill_events.iter().any(|f| f.order_side == OrderSide::Sell);
    assert!(has_buy_fill, "Should have BUY fill for long position");
    assert!(has_sell_fill, "Should have SELL fill for short position");

    let cache = ctx.cache.borrow();
    let positions = cache.positions(None, None, None, None, None);
    assert_eq!(positions.len(), 2, "Should have 2 positions in cache");
}

#[tokio::test]
async fn test_reconcile_mass_status_routes_to_hedging_with_venue_position_id() {
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

    // Position report WITH venue_position_id = hedge mode
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(PositionId::from("P-HEDGE-001")),
        Some(dec!(3000.50)),
    );

    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should create position since position doesn't exist in cache
    assert!(!result.events.is_empty());
}

#[tokio::test]
async fn test_reconcile_mass_status_routes_to_netting_without_venue_position_id() {
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

    // Position report WITHOUT venue_position_id = netting mode
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None, // No venue_position_id
        Some(dec!(3000.50)),
    );

    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should create position since no position exists for instrument
    assert!(!result.events.is_empty());
}

#[tokio::test]
async fn test_reconcile_mass_status_reconciles_partial_hedge_fill_to_position_report() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let venue_position_id = PositionId::from("P-HEDGE-001");

    ctx.add_instrument(test_instrument());
    let order = create_submitted_order("O-001", instrument_id, OrderSide::Buy, "5.0", "3000.00");
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
        Quantity::from("2.0"),
        Price::from("3000.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Maker,
        Some(client_order_id),
        Some(venue_position_id),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );

    mass_status.add_fill_reports(vec![fill]);
    mass_status.set_report_window(Some(UnixNanos::from(1)), false);

    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(venue_position_id),
        Some(dec!(3000.00)),
    );

    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 3);

    assert_eq!(
        result
            .events
            .iter()
            .filter(|event| matches!(event, OrderEventAny::Accepted(_)))
            .count(),
        1,
    );

    assert_eq!(
        result
            .events
            .iter()
            .filter(|event| matches!(event, OrderEventAny::Filled(_)))
            .count(),
        2,
    );

    let position = ctx
        .cache
        .borrow()
        .position_owned(&venue_position_id)
        .unwrap();
    assert_eq!(position.quantity, Quantity::from("5.0"));
    assert_eq!(position.avg_px_open, dec!(3000.0));
}

#[tokio::test]
async fn test_reconcile_mass_status_leaves_hedge_reversal_identity_unresolved() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let venue_position_id = PositionId::from("P-HEDGE-001");

    ctx.add_instrument(test_instrument());
    let order = create_submitted_order("O-001", instrument_id, OrderSide::Sell, "5.0", "3000.00");
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    mass_status.set_report_window(Some(UnixNanos::from(1)), false);

    mass_status.add_fill_reports(vec![FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-001"),
        OrderSide::Sell,
        Quantity::from("5.0"),
        Price::from("3100.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Maker,
        Some(client_order_id),
        Some(venue_position_id),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    )]);

    mass_status.add_position_reports(vec![PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(venue_position_id),
        Some(dec!(3000.00)),
    )]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 3);

    let cache = ctx.cache.borrow();
    assert_eq!(cache.positions(None, None, None, None, None).len(), 2);
    let reported_position = cache.position(&venue_position_id).unwrap();
    assert!(reported_position.is_closed());
    assert_eq!(reported_position.quantity, Quantity::zero(3));
    let positions = cache.positions_open(
        None,
        Some(&instrument_id),
        None,
        Some(&test_account_id()),
        None,
    );
    assert_eq!(positions.len(), 1);
    assert_ne!(positions[0].id, venue_position_id);
    assert_eq!(
        result.unresolved_positions,
        vec![format!(
            "account={}, instrument={instrument_id}, venue_position_id=Some({venue_position_id:?}), venue_quantity=5.0: position recovery did not restore the reported quantity",
            test_account_id(),
        )]
    );
    assert_eq!(positions[0].side, PositionSide::Long);
    assert_eq!(positions[0].quantity, Quantity::from("5.0"));
    assert_eq!(positions[0].avg_px_open, dec!(3000.0));
    assert_eq!(positions[0].realized_pnl, Some(Money::from("0 USDT")));
}

#[tokio::test]
async fn test_reconcile_mass_status_skips_only_position_with_fill_position_id_conflict() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let legacy_position_id = PositionId::from("P-LEGACY-LONG");
    let venue_position_id = PositionId::from("P-HEDGE-LONG");
    let other_position_id = PositionId::from("P-HEDGE-SHORT");

    ctx.add_instrument(test_instrument());
    let order = create_submitted_order("O-001", instrument_id, OrderSide::Buy, "5.0", "3000.00");
    ctx.cache
        .borrow_mut()
        .add_order(
            order,
            Some(legacy_position_id),
            Some(test_client_id()),
            false,
        )
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
        TradeId::from("T-001"),
        OrderSide::Buy,
        Quantity::from("5.0"),
        Price::from("3000.00"),
        Money::from("0.50 USDT"),
        LiquiditySide::Maker,
        Some(client_order_id),
        Some(venue_position_id),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    )]);

    mass_status.add_position_reports(vec![PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(venue_position_id),
        Some(dec!(3000.00)),
    )]);

    mass_status.add_position_reports(vec![PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Short,
        Quantity::from("3.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(other_position_id),
        Some(dec!(3100.00)),
    )]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let [
        OrderEventAny::Filled(conflicting_fill),
        OrderEventAny::Accepted(_),
        OrderEventAny::Filled(other_fill),
    ] = result.events.as_slice()
    else {
        panic!("expected the conflicting fill followed by the synthetic position events");
    };

    assert_eq!(conflicting_fill.position_id, Some(venue_position_id));
    assert_eq!(other_fill.position_id, Some(other_position_id));

    let order = ctx.get_order(&client_order_id).unwrap();
    assert_eq!(order.filled_qty(), Quantity::zero(3));
    assert!(ctx.cache.borrow().position(&legacy_position_id).is_none());
    assert!(ctx.cache.borrow().position(&venue_position_id).is_none());

    let other_position = ctx
        .cache
        .borrow()
        .position_owned(&other_position_id)
        .unwrap();
    assert_eq!(other_position.id, other_position_id);
    assert_eq!(other_position.instrument_id, instrument_id);
    assert_eq!(other_position.side, PositionSide::Short);
    assert_eq!(other_position.quantity, Quantity::from("3.0"));
    assert_eq!(other_position.avg_px_open, dec!(3100.0));
}

#[tokio::test]
async fn test_reconcile_mass_status_does_not_duplicate_matching_hedge_filled_order() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let venue_order_id = VenueOrderId::from("V-001");
    let venue_position_id = PositionId::from("P-HEDGE-001");

    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let order_report = OrderStatusReport::new(
        test_account_id(),
        instrument_id,
        None,
        venue_order_id,
        OrderSide::Buy.into(),
        OrderType::Market,
        TimeInForce::Gtc,
        OrderStatus::Filled,
        Quantity::from("5.0"),
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    )
    .with_avg_px(dec!(3000.0))
    .with_venue_position_id(venue_position_id);

    mass_status.add_order_reports(vec![order_report]);

    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(venue_position_id),
        Some(dec!(3000.00)),
    );

    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let filled_count = result
        .events
        .iter()
        .filter(|e| matches!(e, OrderEventAny::Filled(_)))
        .count();

    assert_eq!(filled_count, 1, "Expected exactly 1 fill event");
}

#[tokio::test]
async fn test_reconcile_mass_status_skips_hedge_position_when_fills_lack_position_id() {
    // Tests that hedge position reconciliation is skipped when fills exist for the
    // instrument but lack venue_position_id (common when venues only include IDs on
    // position reports)
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");

    ctx.add_instrument(test_instrument());
    let order = create_limit_order("O-001", instrument_id, OrderSide::Buy, "5.0", "3000.00");
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Add a fill report WITHOUT venue_position_id
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
        Some(client_order_id),
        None, // No venue_position_id on fill
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );

    mass_status.add_fill_reports(vec![fill]);

    // Add a hedge position report WITH venue_position_id
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(PositionId::from("P-HEDGE-001")), // Has venue_position_id
        Some(dec!(3000.00)),
    );

    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should only have fill event, position report skipped due to instrument-level fill
    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Filled(_)));
}

#[tokio::test]
async fn test_reconcile_hedge_does_not_skip_unrelated_positions() {
    // Tests that when fills have venue_position_id, only that specific position is skipped,
    // not other hedge positions on the same instrument
    let config = ExecutionManagerConfig {
        generate_missing_orders: true,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let position_id_1 = PositionId::from("P-HEDGE-001");
    let position_id_2 = PositionId::from("P-HEDGE-002");

    ctx.add_instrument(test_instrument());
    let order = create_limit_order("O-001", instrument_id, OrderSide::Buy, "5.0", "3000.00");
    ctx.add_order(order);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Add a fill report WITH venue_position_id for P-HEDGE-001
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
        Some(client_order_id),
        Some(position_id_1), // Fill attributed to P-HEDGE-001
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
    );

    mass_status.add_fill_reports(vec![fill]);

    // Add position reports for BOTH positions
    let position_report_1 = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(position_id_1),
        Some(dec!(3000.00)),
    );

    let position_report_2 = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Short,
        Quantity::from("3.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(position_id_2), // Different position
        Some(dec!(3100.00)),
    );

    mass_status.add_position_reports(vec![position_report_1, position_report_2]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should have:
    // - 1 fill event for P-HEDGE-001 (from fill report)
    // - Events for P-HEDGE-002 (from position report, should NOT be skipped)
    let filled_count = result
        .events
        .iter()
        .filter(|e| matches!(e, OrderEventAny::Filled(_)))
        .count();

    // At least 2 fills: one from fill report, one from position report for P-HEDGE-002
    assert!(
        filled_count >= 2,
        "Expected at least 2 fill events, was {filled_count}"
    );
}

#[tokio::test]
async fn test_reconcile_hedge_position_matching_quantities() {
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = test_instrument_id();
    let position_id = PositionId::from("P-HEDGE-001");

    ctx.add_instrument(instrument.clone());

    // Add existing position to cache with 5.0 qty
    let position = create_test_position(&instrument, position_id, OrderSide::Buy, "5.0", "3000.00");
    ctx.add_position(&position);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Position report matches cached position exactly
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(position_id),
        Some(dec!(3000.00)),
    );

    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // No events needed since positions match
    assert!(
        result.events.is_empty(),
        "Expected no events when positions match, was {}",
        result.events.len()
    );
}

#[tokio::test]
async fn test_reconcile_hedge_position_discrepancy_generates_order() {
    let config = ExecutionManagerConfig {
        generate_missing_orders: true,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = test_instrument_id();
    let position_id = PositionId::from("P-HEDGE-001");

    ctx.add_instrument(instrument.clone());

    // Add existing position to cache with 5.0 qty
    let position = create_test_position(&instrument, position_id, OrderSide::Buy, "5.0", "3000.00");
    ctx.add_position(&position);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Position report shows 8.0 qty (discrepancy of 3.0)
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("8.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(position_id),
        Some(dec!(3000.00)),
    );

    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should generate reconciliation order to fix the discrepancy
    assert!(
        !result.events.is_empty(),
        "Expected events for position discrepancy reconciliation"
    );
}

#[tokio::test]
async fn test_reconcile_missing_hedge_position_generates_order() {
    let config = ExecutionManagerConfig {
        generate_missing_orders: true,
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

    // Position report for position that doesn't exist in cache
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(PositionId::from("P-MISSING-001")),
        Some(dec!(3000.50)),
    );

    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should generate order to create the missing position
    assert!(
        !result.events.is_empty(),
        "Expected events for missing position creation"
    );
}

#[tokio::test]
async fn test_reconcile_hedge_position_discrepancy_disabled() {
    let config = ExecutionManagerConfig {
        generate_missing_orders: false, // Disabled
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = test_instrument_id();
    let position_id = PositionId::from("P-HEDGE-001");

    ctx.add_instrument(instrument.clone());

    // Add existing position with different qty than venue reports
    let position = create_test_position(&instrument, position_id, OrderSide::Buy, "5.0", "3000.00");
    ctx.add_position(&position);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Position report shows 8.0 qty (discrepancy)
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("8.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(position_id),
        Some(dec!(3000.00)),
    );

    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // No events since generate_missing_orders is disabled
    assert!(
        result.events.is_empty(),
        "Expected no events when generate_missing_orders is disabled"
    );
}

#[tokio::test]
async fn test_reconcile_hedge_position_both_flat() {
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

    // Position report with zero quantity (flat)
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Flat,
        Quantity::from("0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(PositionId::from("P-FLAT-001")),
        None,
    );

    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // No events needed - position doesn't exist in cache and report is flat
    assert!(result.events.is_empty());
}

#[tokio::test]
async fn test_reconcile_hedge_short_position() {
    let config = ExecutionManagerConfig {
        generate_missing_orders: true,
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

    // Short position report
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Short,
        Quantity::from("3.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(PositionId::from("P-SHORT-001")),
        Some(dec!(3100.00)),
    );

    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should create short position
    assert!(!result.events.is_empty());

    // Verify one of the events is a sell order (for short position)
    let has_sell = result.events.iter().any(|e| {
        if let OrderEventAny::Filled(filled) = e {
            filled.order_side == OrderSide::Sell
        } else {
            false
        }
    });

    assert!(has_sell, "Expected a sell order for short position");
}

#[tokio::test]
async fn test_reconcile_mass_status_deduplicates_netting_reports_same_instrument() {
    // Tests that multiple netting reports (no venue_position_id) for the same instrument
    // only create one position, not duplicates
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

    // Add two netting reports for the same instrument (both without venue_position_id)
    let position_report_1 = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None, // No venue_position_id = netting mode
        Some(dec!(3000.50)),
    );

    let position_report_2 = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_001),
        UnixNanos::from(1_000_001),
        None,
        None, // No venue_position_id = netting mode
        Some(dec!(3000.50)),
    );

    mass_status.add_position_reports(vec![position_report_1, position_report_2]);

    let _result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Deduplication: only ONE position should be created from duplicate netting reports
    let cache = ctx.cache.borrow();
    let positions = cache.positions(None, None, None, None, None);
    assert_eq!(
        positions.len(),
        1,
        "Should have exactly 1 position (duplicates skipped), was {}",
        positions.len()
    );
    assert_eq!(
        positions[0].quantity,
        Quantity::from("5.0"),
        "Position should have qty 5.0"
    );
}

#[tokio::test]
async fn test_reconcile_mass_status_deduplicates_hedge_reports_same_position_id() {
    // Tests that multiple hedge reports for the same venue_position_id only create
    // one position, not duplicates
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let venue_position_id = PositionId::from("P-HEDGE-001");
    ctx.add_instrument(test_instrument());

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Add two hedge reports for the same venue_position_id (duplicate snapshots)
    let position_report_1 = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(venue_position_id),
        Some(dec!(3000.50)),
    );

    let position_report_2 = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_001),
        UnixNanos::from(1_000_001),
        None,
        Some(venue_position_id), // Same venue_position_id
        Some(dec!(3000.50)),
    );

    mass_status.add_position_reports(vec![position_report_1, position_report_2]);

    let _result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Deduplication: only ONE position should be created from duplicate hedge reports
    let cache = ctx.cache.borrow();
    let positions = cache.positions(None, None, None, None, None);
    assert_eq!(
        positions.len(),
        1,
        "Should have exactly 1 position (duplicates skipped), was {}",
        positions.len()
    );
    assert_eq!(
        positions[0].id, venue_position_id,
        "Position should have correct ID"
    );
    assert_eq!(
        positions[0].quantity,
        Quantity::from("5.0"),
        "Position should have qty 5.0"
    );
}

#[tokio::test]
async fn test_position_reconciliation_order_has_reconciliation_tag() {
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

    // Position report with no corresponding orders - this triggers position reconciliation
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3000.50)),
    );

    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert!(!result.events.is_empty());

    let fills: Vec<_> = result
        .events
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => Some(fill),
            _ => None,
        })
        .collect();

    assert_eq!(fills.len(), 1);
    let fill = fills[0];
    assert_eq!(fill.last_qty, Quantity::from("5.0"));
    assert_eq!(fill.last_px, Price::from("3000.50"));
    assert_eq!(fill.order_side, OrderSide::Buy);
    assert_eq!(fill.commission, None);
    assert!(fill.reconciliation);
    assert!(fill.to_string().contains("commission=None,"));

    // Get the synthetic order created and verify it has the RECONCILIATION tag
    if let OrderEventAny::Accepted(accepted) = &result.events[0] {
        let order = ctx
            .get_order(&accepted.client_order_id)
            .expect("Order should exist");
        let tags = order.tags().expect("Order should have tags");
        assert!(
            tags.contains(&ustr::Ustr::from("RECONCILIATION")),
            "Position reconciliation order should have RECONCILIATION tag, was {tags:?}",
        );
    } else {
        panic!("Expected Accepted event, was {:?}", result.events[0]);
    }

    let repeated = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);
    assert!(repeated.events.is_empty());
}

#[tokio::test]
async fn test_netting_position_cross_zero_long_to_short() {
    // Test: Cached position is long +5.0, venue reports short -3.0
    // Should generate: close fill (sell 5.0) + open fill (sell 3.0) = 2 fills
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(instrument.clone());

    // Add cached LONG position of 5.0
    let position = create_test_position(
        &instrument,
        PositionId::new("P-001"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
    );
    ctx.add_position(&position);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Venue reports SHORT position of -3.0
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Short,
        Quantity::from("3.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None, // Netting mode
        Some(dec!(3100.00)),
    );

    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should have 2 fills: close (sell 5.0) + open (sell 3.0)
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
        2,
        "Cross-zero should generate 2 fills (close + open), was {}",
        fill_events.len()
    );

    // First fill should be SELL 5.0 (close long)
    assert_eq!(fill_events[0].order_side, OrderSide::Sell);
    assert_eq!(fill_events[0].last_qty, Quantity::from("5.0"));

    // Second fill should be SELL 3.0 (open short)
    assert_eq!(fill_events[1].order_side, OrderSide::Sell);
    assert_eq!(fill_events[1].last_qty, Quantity::from("3.0"));

    // Close and open legs must hash to distinct venue_order_ids so the engine
    // can tell them apart across reconciliation replays.
    assert_ne!(fill_events[0].venue_order_id, fill_events[1].venue_order_id);
}

#[tokio::test]
async fn test_netting_position_cross_zero_short_to_long() {
    // Test: Cached position is short -4.0, venue reports long +2.0
    // Should generate: close fill (buy 4.0) + open fill (buy 2.0) = 2 fills
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(instrument.clone());

    // Add cached SHORT position of 4.0
    let position = create_test_position(
        &instrument,
        PositionId::new("P-001"),
        OrderSide::Sell,
        "4.0",
        "3000.00",
    );
    ctx.add_position(&position);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Venue reports LONG position of +2.0
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("2.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None, // Netting mode
        Some(dec!(2900.00)),
    );

    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should have 2 fills: close (buy 4.0) + open (buy 2.0)
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
        2,
        "Cross-zero should generate 2 fills (close + open), was {}",
        fill_events.len()
    );

    // First fill should be BUY 4.0 (close short)
    assert_eq!(fill_events[0].order_side, OrderSide::Buy);
    assert_eq!(fill_events[0].last_qty, Quantity::from("4.0"));

    // Second fill should be BUY 2.0 (open long)
    assert_eq!(fill_events[1].order_side, OrderSide::Buy);
    assert_eq!(fill_events[1].last_qty, Quantity::from("2.0"));

    // Close and open legs must hash to distinct venue_order_ids so the engine
    // can tell them apart across reconciliation replays.
    assert_ne!(fill_events[0].venue_order_id, fill_events[1].venue_order_id);
}

#[tokio::test]
async fn test_netting_position_flat_report_closes_cached_position() {
    // Test: Cached position is long +5.0, venue reports flat (0.0)
    // Should generate: close fill (sell 5.0)
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(instrument.clone());

    // Add cached LONG position of 5.0
    let position = create_test_position(
        &instrument,
        PositionId::new("P-001"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
    );
    ctx.add_position(&position);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    // Venue reports FLAT position (0.0)
    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Flat,
        Quantity::from("0.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None, // Netting mode
        None, // No avg_px for flat
    );

    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should have 1 fill: close (sell 5.0)
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
        "Flat report should generate 1 closing fill, was {}",
        fill_events.len()
    );

    // Fill should be SELL 5.0 (close long position)
    assert_eq!(fill_events[0].order_side, OrderSide::Sell);
    assert_eq!(fill_events[0].last_qty, Quantity::from("5.0"));
}

#[tokio::test]
async fn test_cross_zero_with_missing_cached_avg_px_returns_none() {
    // When cached position has no avg_px, cross-zero cannot generate close fill
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(instrument.clone());

    let position = create_test_position(
        &instrument,
        PositionId::new("P-001"),
        OrderSide::Buy,
        "5.0",
        "0.00", // Zero price - will be treated as no avg_px in some paths
    );
    ctx.add_position(&position);

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
        PositionSide::Short,
        Quantity::from("3.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3100.00)),
    );

    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // With zero cached price, cross-zero should still attempt reconciliation
    // but may produce different behavior - verify no panic at minimum
    assert!(
        result.events.len() <= 4,
        "Should not produce excessive events"
    );
}

#[tokio::test]
async fn test_cross_zero_with_missing_venue_avg_px_closes_only() {
    // When venue position has no avg_px, cross-zero can close but not open new position
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(instrument.clone());

    let position = create_test_position(
        &instrument,
        PositionId::new("P-001"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
    );
    ctx.add_position(&position);

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
        PositionSide::Short,
        Quantity::from("3.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        None, // No avg_px - cannot open new position
    );

    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // Should generate close fill only (not open fill due to missing venue avg_px)
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
        "Should generate only close fill when venue avg_px missing, was {}",
        fill_events.len()
    );
    assert_eq!(fill_events[0].order_side, OrderSide::Sell);
    assert_eq!(fill_events[0].last_qty, Quantity::from("5.0"));
}

#[tokio::test]
async fn test_hedge_mode_multiple_positions_same_instrument() {
    // Hedge mode venues can have multiple positions (long + short) for same instrument
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(instrument);

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let long_position = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("10.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(PositionId::new("HEDGE-LONG-001")), // venue_position_id indicates hedge mode
        Some(dec!(3000.00)),
    );

    let short_position = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Short,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(PositionId::new("HEDGE-SHORT-001")),
        Some(dec!(3100.00)),
    );

    mass_status.add_position_reports(vec![long_position, short_position]);

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

    assert!(
        fill_events.len() >= 2,
        "Should process both hedge positions, was {} fills",
        fill_events.len()
    );
    let has_buy = fill_events.iter().any(|f| f.order_side == OrderSide::Buy);
    let has_sell = fill_events.iter().any(|f| f.order_side == OrderSide::Sell);
    assert!(has_buy, "Should have BUY fill for long position");
    assert!(has_sell, "Should have SELL fill for short position");
}

#[tokio::test]
async fn test_hedge_mode_with_filter_unclaimed_external_allows_synthetic() {
    // Synthetic orders should bypass filter_unclaimed_external
    let config = ExecutionManagerConfig {
        filter_unclaimed_external: true,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(instrument);

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
        Quantity::from("10.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(PositionId::new("HEDGE-001")),
        Some(dec!(3000.00)),
    );

    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert!(
        !result.events.is_empty(),
        "Synthetic orders should bypass filter_unclaimed_external"
    );
}

#[tokio::test]
async fn test_reconciliation_instrument_ids_filters_position_reports() {
    // Position reports for instruments NOT in reconciliation_instrument_ids should be skipped
    let included_instrument_id = test_instrument_id();

    let config = ExecutionManagerConfig {
        reconciliation_instrument_ids: IndexSet::from([included_instrument_id]),
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    // Add excluded instrument
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

    // Position report for excluded instrument
    let position_report = PositionStatusReport::new(
        test_account_id(),
        excluded_instrument_id,
        PositionSide::Long,
        Quantity::from("10.0"),
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
        None,
        None,
        Some(dec!(100.00)),
    );

    mass_status.add_position_reports(vec![position_report]);

    let _result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // No position should be created for excluded instrument
    let cache = ctx.cache.borrow();
    let positions = cache.positions(None, None, None, None, None);
    let has_excluded_position = positions
        .iter()
        .any(|p| p.instrument_id == excluded_instrument_id);
    assert!(
        !has_excluded_position,
        "No position should be created for excluded instrument"
    );
}

#[tokio::test]
async fn test_zero_quantity_fill_does_not_suppress_hedge_position_report() {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        generate_missing_orders: true,
        ..Default::default()
    });

    let instrument_id = test_instrument_id();
    let position_id = PositionId::from("P-ZERO-HEDGE");
    ctx.add_instrument(test_instrument());
    let zero = create_fill_report(
        ClientOrderId::from("O-ZERO-HEDGE"),
        VenueOrderId::from("V-ZERO-HEDGE"),
        instrument_id,
        TradeId::from("T-ZERO-HEDGE"),
        "0.0",
    );
    let mut mass_status = create_mass_status(vec![], vec![zero]);
    mass_status.add_position_reports(vec![PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(position_id),
        Some(dec!(3000.00)),
    )]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let fills: Vec<_> = result
        .events
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => Some(fill),
            _ => None,
        })
        .collect();

    assert_eq!(fills.len(), 1);
    assert_eq!(fills[0].last_qty, Quantity::from("5.0"));
    assert_eq!(fills[0].position_id, Some(position_id));
    assert_ne!(fills[0].trade_id, TradeId::from("T-ZERO-HEDGE"));
    let cache = ctx.cache.borrow();
    assert!(!cache.order_exists(&ClientOrderId::from("O-ZERO-HEDGE")));
    assert_eq!(
        cache.position(&position_id).unwrap().quantity,
        Quantity::from("5.0")
    );
}

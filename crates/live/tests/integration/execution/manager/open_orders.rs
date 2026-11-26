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
fn test_check_open_order_queries_builds_query_for_cached_open_order() {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        max_single_order_queries_per_cycle: 5,
        ..Default::default()
    });

    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-QUERY-001");
    let venue_order_id = VenueOrderId::from("V-QUERY-001");
    let client_id = test_client_id();

    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, client_id);

    let queries = ctx.manager.check_open_order_queries();

    assert_eq!(queries.len(), 1);

    match &queries[0] {
        TradingCommand::QueryOrder(query) => {
            assert_eq!(query.client_id, Some(client_id));
            assert_eq!(query.client_order_id, client_order_id);
            assert_eq!(query.venue_order_id, Some(venue_order_id));
        }
        command => panic!("Expected QueryOrder, was {command:?}"),
    }
}

#[rstest]
fn test_check_open_order_queries_dedupes_open_inflight_order() {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        max_single_order_queries_per_cycle: 5,
        single_order_query_delay_ms: 0,
        ..Default::default()
    });

    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-QUERY-002");
    let venue_order_id = VenueOrderId::from("V-QUERY-002");
    let client_id = test_client_id();
    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, client_id);
    let order = ctx
        .cache
        .borrow()
        .order(&client_order_id)
        .map(|order| order.clone())
        .unwrap();
    let pending = OrderEventAny::PendingCancel(
        OrderPendingCancelSpec::builder()
            .trader_id(order.trader_id())
            .strategy_id(order.strategy_id())
            .instrument_id(order.instrument_id())
            .client_order_id(order.client_order_id())
            .account_id(test_account_id())
            .venue_order_id(venue_order_id)
            .build(),
    );
    ctx.cache.borrow_mut().update_order(&pending).unwrap();

    let queries = ctx.manager.check_open_order_queries();

    assert_eq!(queries.len(), 1);

    match &queries[0] {
        TradingCommand::QueryOrder(query) => {
            assert_eq!(query.client_id, Some(client_id));
            assert_eq!(query.client_order_id, client_order_id);
            assert_eq!(query.venue_order_id, Some(venue_order_id));
        }
        command => panic!("Expected QueryOrder, was {command:?}"),
    }
}

#[rstest]
fn test_check_open_order_queries_respects_per_cycle_limit() {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        max_single_order_queries_per_cycle: 1,
        ..Default::default()
    });

    ctx.add_instrument(test_instrument());

    insert_accepted_limit_order(
        &ctx,
        ClientOrderId::from("O-QUERY-003"),
        VenueOrderId::from("V-QUERY-003"),
        test_client_id(),
    );
    insert_accepted_limit_order(
        &ctx,
        ClientOrderId::from("O-QUERY-004"),
        VenueOrderId::from("V-QUERY-004"),
        test_client_id(),
    );

    let queries = ctx.manager.check_open_order_queries();

    assert_eq!(queries.len(), 1);
}

#[rstest]
fn test_check_open_order_queries_rotates_after_open_report_response() {
    fn queried_client_order_id(queries: &[TradingCommand]) -> ClientOrderId {
        match &queries[0] {
            TradingCommand::QueryOrder(query) => query.client_order_id,
            command => panic!("Expected QueryOrder, was {command:?}"),
        }
    }

    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        max_single_order_queries_per_cycle: 1,
        open_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    });

    ctx.add_instrument(test_instrument());

    let first_id = ClientOrderId::from("O-QUERY-011");
    let second_id = ClientOrderId::from("O-QUERY-012");
    let third_id = ClientOrderId::from("O-QUERY-013");
    let first_venue_id = VenueOrderId::from("V-QUERY-011");
    let second_venue_id = VenueOrderId::from("V-QUERY-012");
    let third_venue_id = VenueOrderId::from("V-QUERY-013");

    insert_accepted_limit_order(&ctx, first_id, first_venue_id, test_client_id());
    insert_accepted_limit_order(&ctx, second_id, second_venue_id, test_client_id());
    insert_accepted_limit_order(&ctx, third_id, third_venue_id, test_client_id());

    let first_queries = ctx.manager.check_open_order_queries();
    let first_report = create_order_report(
        Some(first_id),
        first_venue_id,
        test_instrument_id(),
        OrderStatus::Accepted,
        Quantity::from("10.0"),
        Quantity::from("0.0"),
    );
    ctx.manager
        .observe_execution_report(&ExecutionReport::Order(Box::new(first_report)));

    let second_queries = ctx.manager.check_open_order_queries();
    let second_report = create_order_report(
        Some(second_id),
        second_venue_id,
        test_instrument_id(),
        OrderStatus::Accepted,
        Quantity::from("10.0"),
        Quantity::from("0.0"),
    );
    ctx.manager
        .observe_execution_report(&ExecutionReport::Order(Box::new(second_report)));

    let third_queries = ctx.manager.check_open_order_queries();

    assert_eq!(first_queries.len(), 1);
    assert_eq!(second_queries.len(), 1);
    assert_eq!(third_queries.len(), 1);
    assert_eq!(queried_client_order_id(&first_queries), first_id);
    assert_eq!(queried_client_order_id(&second_queries), second_id);
    assert_eq!(queried_client_order_id(&third_queries), third_id);
}

#[rstest]
fn test_check_open_order_queries_returns_empty_when_cycle_limit_is_zero() {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        max_single_order_queries_per_cycle: 0,
        ..Default::default()
    });

    ctx.add_instrument(test_instrument());

    insert_accepted_limit_order(
        &ctx,
        ClientOrderId::from("O-QUERY-005"),
        VenueOrderId::from("V-QUERY-005"),
        test_client_id(),
    );

    let queries = ctx.manager.check_open_order_queries();

    assert!(queries.is_empty());
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_check_open_order_queries_respects_query_delay() {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        max_single_order_queries_per_cycle: 5,
        single_order_query_delay_ms: 100,
        ..Default::default()
    });

    ctx.add_instrument(test_instrument());

    insert_accepted_limit_order(
        &ctx,
        ClientOrderId::from("O-QUERY-006"),
        VenueOrderId::from("V-QUERY-006"),
        test_client_id(),
    );

    let first_queries = ctx.manager.check_open_order_queries();
    let delayed_queries = ctx.manager.check_open_order_queries();
    ctx.advance_time(200_000_000);
    let domain_advanced_queries = ctx.manager.check_open_order_queries();
    advance_clock(dst::time::Duration::from_millis(100)).await;
    let monotonic_advanced_queries = ctx.manager.check_open_order_queries();

    assert_eq!(first_queries.len(), 1);
    assert!(delayed_queries.is_empty());
    assert!(domain_advanced_queries.is_empty());
    assert_eq!(monotonic_advanced_queries.len(), 1);
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_check_open_order_queries_defers_with_recent_local_activity() {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        max_single_order_queries_per_cycle: 5,
        open_check_threshold_ns: DurationNanos::from_secs(5),
        ..Default::default()
    });

    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-QUERY-007");
    insert_accepted_limit_order(
        &ctx,
        client_order_id,
        VenueOrderId::from("V-QUERY-007"),
        test_client_id(),
    );
    ctx.manager.record_local_activity(client_order_id);

    let queries = ctx.manager.check_open_order_queries();
    ctx.advance_time(6_000_000_000);
    let domain_advanced_queries = ctx.manager.check_open_order_queries();
    advance_clock(dst::time::Duration::from_secs(5)).await;
    let monotonic_advanced_queries = ctx.manager.check_open_order_queries();

    assert!(queries.is_empty());
    assert!(domain_advanced_queries.is_empty());
    assert_eq!(monotonic_advanced_queries.len(), 1);
}

#[rstest]
fn test_check_open_order_queries_skips_filtered_client_order_ids() {
    let filtered_id = ClientOrderId::from("O-QUERY-008");

    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        max_single_order_queries_per_cycle: 5,
        filtered_client_order_ids: IndexSet::from([filtered_id]),
        ..Default::default()
    });

    ctx.add_instrument(test_instrument());

    insert_accepted_limit_order(
        &ctx,
        filtered_id,
        VenueOrderId::from("V-QUERY-008"),
        test_client_id(),
    );

    let queries = ctx.manager.check_open_order_queries();

    assert!(queries.is_empty());
}

#[rstest]
fn test_check_open_order_queries_filters_reconciliation_instruments() {
    let included_id = ClientOrderId::from("O-QUERY-009");
    let excluded_id = ClientOrderId::from("O-QUERY-010");

    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        max_single_order_queries_per_cycle: 5,
        reconciliation_instrument_ids: IndexSet::from([test_instrument_id()]),
        ..Default::default()
    });

    ctx.add_instrument(test_instrument());
    ctx.add_instrument(test_instrument2());

    insert_accepted_limit_order(
        &ctx,
        included_id,
        VenueOrderId::from("V-QUERY-009"),
        test_client_id(),
    );
    insert_accepted_limit_order_for_instrument(
        &ctx,
        excluded_id,
        VenueOrderId::from("V-QUERY-010"),
        test_instrument_id2(),
        ClientId::from("BYBIT"),
    );

    let queries = ctx.manager.check_open_order_queries();

    assert_eq!(queries.len(), 1);

    match &queries[0] {
        TradingCommand::QueryOrder(query) => {
            assert_eq!(query.client_order_id, included_id);
            assert_eq!(query.instrument_id, test_instrument_id());
        }
        command => panic!("Expected QueryOrder, was {command:?}"),
    }
}

#[rstest]
#[case::client_id(true)]
#[case::venue_id(false)]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_check_open_orders_defers_with_recent_local_activity(#[case] has_client_id: bool) {
    // Test that reconciliation is deferred when there's recent local activity
    // within the threshold, to avoid race conditions with in-flight fills.
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::from_millis(200), // 200ms threshold
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let instrument_id = test_instrument_id();

    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, test_client_id());

    ctx.manager.record_local_activity(client_order_id);

    let report = create_order_report(
        has_client_id.then_some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("5.0"),
    )
    .with_avg_px(dec!(100.0));

    let mock_client = MockExecutionClient::new(vec![report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_open_orders(&clients).await;

    assert!(
        events.is_empty(),
        "Reconciliation should be deferred with recent local activity"
    );
    let cached_order = ctx.get_order(&client_order_id).unwrap();
    assert_eq!(cached_order.status(), OrderStatus::Accepted);
    assert_eq!(cached_order.filled_qty(), Quantity::from("0.0"));
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_check_open_orders_proceeds_after_threshold_exceeded() {
    // Test that reconciliation proceeds when the local activity is older than
    // the configured threshold.
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::from_millis(200), // 200ms threshold
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
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
    ctx.add_order(order.clone());

    ctx.manager.record_local_activity(client_order_id);
    ctx.advance_both(dst::time::Duration::from_millis(500))
        .await;

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("5.0"),
    )
    .with_avg_px(dec!(100.0));

    let mock_client = MockExecutionClient::new(vec![report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_open_orders(&clients).await;

    assert_eq!(
        events.len(),
        1,
        "Reconciliation should proceed when threshold exceeded"
    );

    if let OrderEventAny::Filled(filled) = &events[0] {
        assert_eq!(filled.last_qty, Quantity::from("5.0"));
    } else {
        panic!("Expected OrderFilled event, was {:?}", events[0]);
    }
}

#[rstest]
#[case::client_id(true)]
#[case::venue_id(false)]
#[tokio::test]
async fn test_check_open_orders_proceeds_without_local_activity(#[case] has_client_id: bool) {
    // Test that reconciliation proceeds normally when there's no recorded
    // local activity for the order.
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::from_millis(200), // 200ms threshold
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let instrument_id = test_instrument_id();

    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, test_client_id());

    let report = create_order_report(
        has_client_id.then_some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("5.0"),
    )
    .with_avg_px(dec!(100.0));

    let mock_client = MockExecutionClient::new(vec![report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_open_orders(&clients).await;

    assert_eq!(
        events.len(),
        1,
        "Reconciliation should proceed without local activity"
    );

    if let OrderEventAny::Filled(filled) = &events[0] {
        assert_eq!(filled.client_order_id, client_order_id);
        assert_eq!(filled.venue_order_id, venue_order_id);
        assert_eq!(filled.last_qty, Quantity::from("5.0"));
    } else {
        panic!("Expected OrderFilled event, was {:?}", events[0]);
    }
}

#[tokio::test]
async fn test_targeted_closing_fill_after_startup_snapshot_does_not_reopen_position() {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        open_check_open_only: false,
        open_check_threshold_ns: DurationNanos::ZERO,
        single_order_query_delay_ms: 0,
        ..Default::default()
    });

    ctx.require_authoritative_fills();
    let instrument = test_instrument();
    ctx.add_instrument(instrument.clone());
    let opening_id = ClientOrderId::from("O-RETAINED-OPENING");
    let closing_id = ClientOrderId::from("O-MISSING-CLOSE");
    let opening_venue = VenueOrderId::from("V-RETAINED-OPENING");
    let closing_venue = VenueOrderId::from("V-MISSING-CLOSE");

    for (client_id, venue_id, side) in [
        (opening_id, opening_venue, OrderSide::Buy),
        (closing_id, closing_venue, OrderSide::Sell),
    ] {
        let order = OrderTestBuilder::new(OrderType::Limit)
            .strategy_id(StrategyId::external())
            .client_order_id(client_id)
            .instrument_id(instrument.id())
            .side(side)
            .quantity(Quantity::from("4.0"))
            .price(Price::from("3000.0"))
            .build();
        ctx.add_order(order.clone());
        ctx.exec_engine
            .borrow_mut()
            .process(&TestOrderEventStubs::submitted(&order, test_account_id()));
        ctx.exec_engine
            .borrow_mut()
            .process(&TestOrderEventStubs::accepted(
                &order,
                test_account_id(),
                venue_id,
            ));
    }

    let opening = create_fill_report(
        opening_id,
        opening_venue,
        instrument.id(),
        TradeId::from("T-RETAINED-OPENING"),
        "4.0",
    );
    ctx.exec_engine.borrow_mut().reconcile_fill_report(&opening);
    let report = create_order_report_for_side(
        Some(closing_id),
        closing_venue,
        instrument.id(),
        OrderSide::Sell,
        OrderStatus::Filled,
        Quantity::from("4.0"),
        Quantity::from("4.0"),
    )
    .with_avg_px(dec!(3000.0));
    let mut closing = create_fill_report(
        closing_id,
        closing_venue,
        instrument.id(),
        TradeId::from("T-MISSING-CLOSE"),
        "4.0",
    );
    closing.order_side = OrderSide::Sell;
    closing.ts_event = UnixNanos::from(1_100_000);
    let mut startup = create_mass_status(vec![report.clone()], Vec::new());
    startup.set_report_window(Some(UnixNanos::from(1_500_000)), true);
    startup.add_position_reports(vec![PositionStatusReport::new(
        test_account_id(),
        instrument.id(),
        PositionSide::Flat,
        Quantity::from("0.0"),
        UnixNanos::from(2_000_000),
        UnixNanos::from(2_000_000),
        None,
        None,
        None,
    )]);
    ctx.advance_time(3_000_000);
    ctx.manager
        .reconcile_execution_mass_status(&startup, &ctx.exec_engine);

    let snapshot = {
        let cache = ctx.cache.borrow();
        assert_eq!(
            cache
                .positions_open(None, Some(&instrument.id()), None, None, None)
                .len(),
            0
        );
        let positions = cache.positions(None, Some(&instrument.id()), None, None, None);
        assert_eq!(positions.len(), 1);
        positions[0].clone()
    };

    let mut client = MockExecutionClient::new(vec![report.clone()])
        .with_order_report(report)
        .with_fill_reports(vec![closing.clone()]);
    client.requires_authoritative_fills = true;
    for _ in 0..2 {
        for event in ctx.manager.check_open_orders(&[&client]).await {
            ctx.exec_engine.borrow_mut().process(&event);
            if matches!(event, OrderEventAny::Filled(_)) {
                ctx.exec_engine.borrow_mut().process(&event);
            }
        }
    }

    let recovered = ctx.get_order(&closing_id).unwrap();
    let cache = ctx.cache.borrow();
    let positions = cache.positions(None, Some(&instrument.id()), None, None, None);
    assert_eq!(recovered.status(), OrderStatus::Filled);
    assert_eq!(recovered.filled_qty(), closing.last_qty);
    assert_eq!(recovered.trade_ids(), vec![&closing.trade_id]);
    assert_eq!(
        recovered.commissions(),
        &IndexMap::from([(closing.commission.currency, closing.commission)])
    );
    assert_eq!(
        cache
            .positions_open(None, Some(&instrument.id()), None, None, None)
            .len(),
        0
    );
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].signed_decimal_qty(), Decimal::ZERO);
    assert_eq!(positions[0].events, snapshot.events);
    assert_eq!(positions[0].realized_pnl, snapshot.realized_pnl);
    assert_eq!(positions[0].commissions(), snapshot.commissions());
}

#[rstest]
#[case::uncached(false, "0.0", false)]
#[case::cached(true, "0.0", false)]
#[case::retained(true, "4.0", false)]
#[case::strategy_cached(true, "0.0", true)]
#[tokio::test]
async fn test_targeted_fill_after_startup_snapshot_projects_order_only(
    #[case] cached: bool,
    #[case] retained_qty: Quantity,
    #[case] strategy_owned: bool,
) {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        open_check_open_only: false,
        open_check_threshold_ns: DurationNanos::ZERO,
        single_order_query_delay_ms: 0,
        ..Default::default()
    });

    let instrument = test_instrument();
    ctx.add_instrument(instrument.clone());
    let client_order_id = ClientOrderId::from("O-STARTUP-GAP");
    let venue_order_id = VenueOrderId::from("V-STARTUP-GAP");
    ctx.require_authoritative_fills();

    if cached {
        let order = OrderTestBuilder::new(OrderType::Limit)
            .strategy_id(if strategy_owned {
                StrategyId::from("S-OWNED")
            } else {
                StrategyId::external()
            })
            .client_order_id(client_order_id)
            .instrument_id(instrument.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("10.0"))
            .price(Price::from("3000.0"))
            .build();

        ctx.add_order(order.clone());
        ctx.exec_engine
            .borrow_mut()
            .process(&TestOrderEventStubs::submitted(&order, test_account_id()));
        ctx.exec_engine
            .borrow_mut()
            .process(&TestOrderEventStubs::accepted(
                &order,
                test_account_id(),
                venue_order_id,
            ));

        if retained_qty.is_positive() {
            let retained = create_fill_report(
                client_order_id,
                venue_order_id,
                instrument.id(),
                TradeId::from("T-RETAINED"),
                "4.0",
            );
            ctx.exec_engine
                .borrow_mut()
                .reconcile_fill_report(&retained);
        }
    }

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument.id(),
        OrderStatus::Filled,
        Quantity::from("10.0"),
        Quantity::from("10.0"),
    )
    .with_avg_px(dec!(3000.0));

    let mut fill = create_fill_report(
        client_order_id,
        venue_order_id,
        instrument.id(),
        TradeId::from("T-STARTUP-GAP"),
        if retained_qty.is_zero() {
            "10.0"
        } else {
            "6.0"
        },
    );

    fill.ts_event = UnixNanos::from(1_100_000);
    let mut startup = create_mass_status(vec![report.clone()], Vec::new());
    startup.set_report_window(Some(UnixNanos::from(1_500_000)), true);
    startup.add_position_reports(vec![PositionStatusReport::new(
        test_account_id(),
        instrument.id(),
        PositionSide::Long,
        Quantity::from("10.0"),
        UnixNanos::from(2_000_000),
        UnixNanos::from(2_000_000),
        None,
        None,
        Some(dec!(3000.0)),
    )]);
    ctx.advance_time(3_000_000);
    ctx.manager
        .reconcile_execution_mass_status(&startup, &ctx.exec_engine);
    let original = ctx.get_order(&client_order_id).unwrap();
    assert_eq!(
        original.status(),
        if retained_qty.is_zero() {
            OrderStatus::Accepted
        } else {
            OrderStatus::PartiallyFilled
        }
    );
    assert_eq!(original.filled_qty(), retained_qty);

    let mut expected_trade_ids = if retained_qty.is_zero() {
        Vec::new()
    } else {
        vec![TradeId::from("T-RETAINED")]
    };

    assert_eq!(
        original
            .trade_ids()
            .into_iter()
            .copied()
            .collect::<Vec<_>>(),
        expected_trade_ids
    );

    let position = {
        let cache = ctx.cache.borrow();
        let positions = cache.positions_open(None, Some(&instrument.id()), None, None, None);
        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].signed_decimal_qty(), dec!(10.0));
        positions[0].clone()
    };

    assert_eq!(
        position.events.last().unwrap().ts_event,
        UnixNanos::from(3_000_000)
    );

    let mut client = MockExecutionClient::new(vec![report.clone()])
        .with_order_report(report)
        .with_fill_reports(vec![fill.clone()]);
    client.requires_authoritative_fills = true;
    for _ in 0..2 {
        let events = ctx.manager.check_open_orders(&[&client]).await;
        for event in &events {
            ctx.exec_engine.borrow_mut().process(event);
            if matches!(event, OrderEventAny::Filled(_)) {
                ctx.exec_engine.borrow_mut().process(event);
            }
        }
    }

    expected_trade_ids.push(fill.trade_id);
    let recovered = ctx.get_order(&client_order_id).unwrap();
    let queries = client.fill_report_queries.borrow();
    let cache = ctx.cache.borrow();
    let positions = cache.positions_open(None, Some(&instrument.id()), None, None, None);
    assert_eq!(queries.len(), 1);
    assert_eq!(queries[0].venue_order_id, Some(venue_order_id));
    assert_eq!(queries[0].start, None);
    assert_eq!(queries[0].end, None);
    assert_eq!(recovered.status(), OrderStatus::Filled);
    assert_eq!(recovered.filled_qty(), Quantity::from("10.0"));
    assert_eq!(
        recovered
            .trade_ids()
            .into_iter()
            .copied()
            .collect::<Vec<_>>(),
        expected_trade_ids
    );
    assert_eq!(
        recovered.commissions(),
        &IndexMap::from([(
            fill.commission.currency,
            if retained_qty.is_zero() {
                fill.commission
            } else {
                Money::from("1.00 USDT")
            }
        )])
    );
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].id, position.id);
    assert_eq!(positions[0].signed_decimal_qty(), dec!(10.0));
    assert_eq!(positions[0].events, position.events);
    assert_eq!(positions[0].realized_pnl, position.realized_pnl);
    assert_eq!(positions[0].commissions(), position.commissions());
}

#[rstest]
#[case::bulk("bulk", false, "2.0")]
#[case::bulk_inflight("bulk", true, "2.0")]
#[case::bulk_stale("bulk", false, "1.0")]
#[case::targeted("targeted", false, "2.0")]
#[case::targeted_inflight("targeted", true, "2.0")]
#[case::targeted_stale("targeted", false, "1.0")]
#[case::startup("startup", false, "2.0")]
#[tokio::test]
async fn test_terminal_reconciliation_and_stream_fill_apply_once(
    #[case] route: &str,
    #[case] stream_inflight: bool,
    #[case] reported_qty: Quantity,
    #[values(OrderStatus::Canceled, OrderStatus::Expired)] status: OrderStatus,
    #[values(false, true)] stream_first: bool,
) {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        open_check_open_only: false,
        open_check_missing_retries: 1,
        open_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    });

    let instrument = test_instrument();
    ctx.add_instrument(instrument.clone());
    let client_order_id = ClientOrderId::from("O-TERMINAL-STREAM");
    let venue_order_id = VenueOrderId::from("V-TERMINAL-STREAM");
    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, test_client_id());
    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument.id(),
        status,
        Quantity::from("10.0"),
        reported_qty,
    )
    .with_price(Price::from("100.0"))
    .with_avg_px(dec!(3000.0));
    let fill = create_fill_report(
        client_order_id,
        venue_order_id,
        instrument.id(),
        TradeId::from("T-TERMINAL-STREAM"),
        "2.0",
    );
    let order = ctx.get_order(&client_order_id).unwrap();
    let streamed = OrderFilledTestBuilder::new(&order, &instrument)
        .trade_id(fill.trade_id)
        .last_qty(fill.last_qty)
        .last_px(fill.last_px)
        .liquidity_side(fill.liquidity_side)
        .commission(fill.commission)
        .ts_event(fill.ts_event)
        .without_position_id()
        .build();

    if stream_first {
        ctx.exec_engine.borrow_mut().process(&streamed);
    }

    let client = MockExecutionClient::new(if route == "bulk" {
        vec![report.clone()]
    } else {
        Vec::new()
    })
    .with_order_report(report.clone())
    .with_fill_reports(vec![fill.clone()]);

    if stream_inflight {
        let engine = ctx.exec_engine.clone();
        let fill = streamed.clone();
        *client.on_fill_reports_query.borrow_mut() = Some(Box::new(move || {
            engine.borrow_mut().process(&fill);
        }));
    }

    if route == "startup" {
        ctx.manager.reconcile_execution_mass_status(
            &create_mass_status(vec![report.clone()], vec![fill.clone()]),
            &ctx.exec_engine,
        );
    } else {
        let events = ctx.manager.check_open_orders(&[&client]).await;
        for event in events {
            ctx.exec_engine.borrow_mut().process(&event);
        }
    }

    ctx.exec_engine.borrow_mut().process(&streamed);
    ctx.exec_engine.borrow_mut().process(&streamed);
    let recovered = ctx.get_order(&client_order_id).unwrap();
    let repeated_client = MockExecutionClient::new(vec![report]).with_fill_reports(vec![fill]);
    let repeated = ctx.manager.check_open_orders(&[&repeated_client]).await;
    for event in &repeated {
        ctx.exec_engine.borrow_mut().process(event);
    }

    let reconciled = ctx.get_order(&client_order_id).unwrap();
    for _ in 0..2 {
        let events = ctx.manager.check_open_orders(&[&repeated_client]).await;
        for event in events {
            ctx.exec_engine.borrow_mut().process(&event);
        }
    }

    let after_repeated = ctx.get_order(&client_order_id).unwrap();
    assert_eq!(after_repeated.events(), reconciled.events());

    let additional = OrderFilledTestBuilder::new(&recovered, &instrument)
        .trade_id(TradeId::from("T-TERMINAL-ADDITIONAL"))
        .last_qty(Quantity::from("1.0"))
        .last_px(Price::from("3030.0"))
        .commission(Money::from("0.25 USDT"))
        .ts_event(UnixNanos::from(2_000_000))
        .without_position_id()
        .build();

    ctx.exec_engine.borrow_mut().process(&additional);

    let final_order = ctx.get_order(&client_order_id).unwrap();
    let cache = ctx.cache.borrow();
    let position_id = cache.position_id(&client_order_id).unwrap();
    let position = cache.position(position_id).unwrap();

    assert_eq!(recovered.status(), status);
    assert_eq!(recovered.filled_qty(), Quantity::from("2.0"));
    assert_eq!(
        recovered.trade_ids(),
        vec![&TradeId::from("T-TERMINAL-STREAM")]
    );
    assert_eq!(
        recovered.commissions().get(&Currency::USDT()),
        Some(&Money::from("0.50 USDT"))
    );
    assert_eq!(
        repeated.len(),
        usize::from(reported_qty < recovered.filled_qty()),
    );

    for event in &repeated {
        assert!(matches!(
            (status, event),
            (OrderStatus::Canceled, OrderEventAny::Canceled(_))
                | (OrderStatus::Expired, OrderEventAny::Expired(_))
        ));
    }

    assert_eq!(final_order.status(), status);

    let quantity = Quantity::from("3.0");
    let price = dec!(3010.0);
    let commission = Money::from("0.75 USDT");

    assert_eq!(final_order.events().len(), reconciled.events().len() + 1);
    assert_eq!(final_order.filled_qty(), quantity);
    assert_eq!(final_order.avg_px(), Some(price));
    assert_eq!(
        final_order.commissions().get(&Currency::USDT()),
        Some(&commission)
    );
    assert_eq!(position.quantity, quantity);
}

#[rstest]
#[case::bulk_empty("bulk", "empty")]
#[case::bulk_partial("bulk", "partial")]
#[case::bulk_failed("bulk", "failed")]
#[case::bulk_foreign("bulk", "foreign")]
#[case::bulk_account("bulk", "account")]
#[case::bulk_instrument("bulk", "instrument")]
#[case::bulk_side("bulk", "side")]
#[case::targeted_empty("targeted", "empty")]
#[case::targeted_zero("targeted", "zero")]
#[case::targeted_partial("targeted", "partial")]
#[case::targeted_failed("targeted", "failed")]
#[case::targeted_foreign("targeted", "foreign")]
#[case::targeted_account("targeted", "account")]
#[case::targeted_instrument("targeted", "instrument")]
#[case::targeted_side("targeted", "side")]
#[case::startup_empty("startup", "empty")]
#[case::startup_partial("startup", "partial")]
#[tokio::test]
async fn test_terminal_reconciliation_defers_unexplained_fill_gap(
    #[case] route: &str,
    #[case] response: &str,
) {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        open_check_open_only: false,
        open_check_missing_retries: 1,
        open_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    });

    ctx.add_instrument(test_instrument());
    let client_order_id = ClientOrderId::from("O-TERMINAL-GAP");
    let venue_order_id = VenueOrderId::from("V-TERMINAL-GAP");
    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, test_client_id());
    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        test_instrument_id(),
        OrderStatus::Canceled,
        Quantity::from("10.0"),
        Quantity::from("2.0"),
    )
    .with_price(Price::from("100.0"))
    .with_avg_px(dec!(3000.0));
    let first = create_fill_report(
        client_order_id,
        venue_order_id,
        test_instrument_id(),
        TradeId::from("T-GAP-1"),
        "1.0",
    );
    let second = create_fill_report(
        client_order_id,
        venue_order_id,
        test_instrument_id(),
        TradeId::from("T-GAP-2"),
        "1.0",
    );

    let initial_fills = match response {
        "zero" => {
            let mut zero = first.clone();
            zero.last_qty = Quantity::zero(1);
            zero.commission = Money::from("123.45 USDT");
            vec![zero]
        }
        "partial" => vec![first.clone()],
        "foreign" | "account" | "instrument" | "side" => {
            let mut foreign = first.clone();
            match response {
                "account" => foreign.account_id = AccountId::from("OTHER-001"),
                "instrument" => foreign.instrument_id = InstrumentId::from("BTCUSDT.BINANCE"),
                "side" => foreign.order_side = OrderSide::Sell,
                _ => foreign.venue_order_id = VenueOrderId::from("V-ANOTHER-ORDER"),
            }

            vec![foreign]
        }
        _ => Vec::new(),
    };

    let client = MockExecutionClient::new(if route == "bulk" {
        vec![report.clone()]
    } else {
        Vec::new()
    })
    .with_order_report(report.clone())
    .with_fill_reports(initial_fills.clone());

    client.fail_fill_reports.set(response == "failed");

    if route == "startup" {
        ctx.manager.reconcile_execution_mass_status(
            &create_mass_status(vec![report.clone()], initial_fills),
            &ctx.exec_engine,
        );
    } else {
        let events = ctx.manager.check_open_orders(&[&client]).await;

        if response == "zero" {
            assert!(events.is_empty());
        }

        for event in events {
            ctx.exec_engine.borrow_mut().process(&event);
        }
    }

    let deferred = ctx.get_order(&client_order_id).unwrap();

    let retry = MockExecutionClient::new(if route == "bulk" {
        vec![report.clone()]
    } else {
        Vec::new()
    })
    .with_order_report(report.clone())
    .with_fill_reports(vec![first.clone(), second.clone()]);

    if route == "startup" {
        ctx.manager.reconcile_execution_mass_status(
            &create_mass_status(vec![report], vec![first, second]),
            &ctx.exec_engine,
        );
    } else {
        let events = ctx.manager.check_open_orders(&[&retry]).await;
        for event in events {
            ctx.exec_engine.borrow_mut().process(&event);
        }
    }

    let recovered = ctx.get_order(&client_order_id).unwrap();
    let partial = response == "partial";

    assert_eq!(
        deferred.status(),
        if partial {
            OrderStatus::PartiallyFilled
        } else {
            OrderStatus::Accepted
        }
    );
    assert_eq!(
        deferred.filled_qty(),
        Quantity::from(if partial { "1.0" } else { "0.0" })
    );

    if response == "zero" {
        assert!(deferred.trade_ids().is_empty());
        assert!(deferred.commissions().is_empty());
    }

    assert_eq!(recovered.status(), OrderStatus::Canceled);
    assert_eq!(recovered.filled_qty(), Quantity::from("2.0"));
    assert_eq!(
        recovered.trade_ids(),
        vec![&TradeId::from("T-GAP-1"), &TradeId::from("T-GAP-2")]
    );
    assert_eq!(
        recovered.commissions().get(&Currency::USDT()),
        Some(&Money::from("1.00 USDT"))
    );

    if route != "startup" {
        let queries = client.fill_report_queries.borrow();
        assert_eq!(queries.len(), 1);
        assert_eq!(queries[0].instrument_id, Some(test_instrument_id()));
        assert_eq!(queries[0].venue_order_id, Some(venue_order_id));
        assert_eq!(queries[0].start, None);
        assert_eq!(queries[0].end, None);
        assert_eq!(retry.fill_report_queries.borrow().len(), 1);
    }
}

#[rstest]
#[case::bulk_canceled(false, false, OrderStatus::Canceled, "4.5", OrderStatus::Canceled, 2)]
#[case::bulk_expired(false, false, OrderStatus::Expired, "4.5", OrderStatus::Expired, 2)]
#[case::targeted_canceled(true, false, OrderStatus::Canceled, "4.5", OrderStatus::Canceled, 2)]
#[case::targeted_expired(true, false, OrderStatus::Expired, "4.5", OrderStatus::Expired, 2)]
#[case::targeted_filled(true, false, OrderStatus::Filled, "10.0", OrderStatus::Filled, 1)]
#[case::bulk_canceled_full(false, false, OrderStatus::Canceled, "10.0", OrderStatus::Filled, 1)]
#[case::bulk_expired_full(false, false, OrderStatus::Expired, "10.0", OrderStatus::Filled, 1)]
#[case::open_only_canceled(true, true, OrderStatus::Canceled, "4.5", OrderStatus::Canceled, 2)]
#[case::open_only_filled(true, true, OrderStatus::Filled, "10.0", OrderStatus::Filled, 1)]
#[tokio::test]
async fn test_check_open_orders_terminal_report_applies_fills(
    #[case] targeted: bool,
    #[case] open_only: bool,
    #[case] report_status: OrderStatus,
    #[case] filled_qty: Quantity,
    #[case] expected_status: OrderStatus,
    #[case] event_count: usize,
) {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        open_check_open_only: open_only,
        single_order_query_delay_ms: 0,
        open_check_missing_retries: 1,
        ..Default::default()
    });

    ctx.add_instrument(test_instrument());
    let client_order_id = ClientOrderId::from("O-TERMINAL-FILL");
    let venue_order_id = VenueOrderId::from("V-TERMINAL-FILL");
    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, test_client_id());
    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        test_instrument_id(),
        report_status,
        Quantity::from("10.0"),
        filled_qty,
    )
    .with_price(Price::from("100.0"))
    .with_avg_px(dec!(101.25));
    let commission = Money::from("1.23 USDT");
    let mut reported_fill = create_fill_report(
        client_order_id,
        venue_order_id,
        test_instrument_id(),
        TradeId::from("T-TERMINAL-REPORTED"),
        "1.0",
    );
    reported_fill.last_qty = filled_qty;
    reported_fill.last_px = Price::from("101.25");
    reported_fill.commission = commission;

    let mut client = if targeted {
        MockExecutionClient::new(Vec::new()).with_order_report(report.clone())
    } else {
        MockExecutionClient::new(vec![report.clone()])
    }
    .with_fill_reports(vec![reported_fill]);

    client.requires_authoritative_fills = report_status == OrderStatus::Filled;
    client.fail_fill_reports.set(true);

    let failed = ctx.manager.check_open_orders(&[&client]).await;

    assert!(failed.is_empty());
    assert_eq!(
        ctx.get_order(&client_order_id).unwrap().status(),
        OrderStatus::Accepted
    );

    client.fail_fill_reports.set(false);
    let events = ctx.manager.check_open_orders(&[&client]).await;
    for event in &events {
        ctx.exec_engine.borrow_mut().process(event);
    }

    let order = ctx.get_order(&client_order_id).unwrap();
    let repeated_client = MockExecutionClient::new(vec![report]);
    repeated_client.fail_fill_reports.set(true);
    let repeated = ctx.manager.check_open_orders(&[&repeated_client]).await;

    assert_eq!(events.len(), event_count);

    let OrderEventAny::Filled(fill) = &events[0] else {
        panic!("Expected fill before terminal event, was {:?}", events[0]);
    };

    assert_eq!(fill.trader_id, order.trader_id());
    assert_eq!(fill.strategy_id, order.strategy_id());
    assert_eq!(fill.instrument_id, test_instrument_id());
    assert_eq!(fill.client_order_id, client_order_id);
    assert_eq!(fill.venue_order_id, venue_order_id);
    assert_eq!(fill.account_id, test_account_id());
    assert_eq!(fill.order_side, OrderSide::Buy);
    assert_eq!(fill.order_type, OrderType::Limit);
    assert_eq!(fill.last_qty, filled_qty);
    assert_eq!(fill.last_px, Price::from("101.25"));
    assert_eq!(fill.commission, Some(commission));
    assert_eq!(fill.trade_id, TradeId::from("T-TERMINAL-REPORTED"));
    assert_eq!(order.status(), expected_status);
    assert_eq!(order.filled_qty(), filled_qty);
    assert_eq!(
        order.commissions().get(&Currency::USDT()),
        Some(&commission)
    );
    assert!(repeated.is_empty());
}

#[rstest]
#[case::client_id(true)]
#[case::venue_id(false)]
#[tokio::test]
async fn test_check_open_orders_skips_unknown_report_and_processes_next(
    #[case] has_client_id: bool,
) {
    let mut ctx = TestContext::new();
    ctx.add_instrument(test_instrument());
    let client_order_id = ClientOrderId::from("O-KNOWN");
    let venue_order_id = VenueOrderId::from("V-KNOWN");
    let unknown_client_order_id = ClientOrderId::from("O-UNKNOWN");
    let unknown_venue_order_id = VenueOrderId::from("V-UNKNOWN");
    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, test_client_id());
    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        test_instrument_id(),
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("4.5"),
    )
    .with_avg_px(dec!(101.25));
    let mut unknown_report = report.clone();
    unknown_report.client_order_id = has_client_id.then_some(unknown_client_order_id);
    unknown_report.venue_order_id = unknown_venue_order_id;
    let commission = Money::from("1.23 USDT");
    let client = MockExecutionClient::new(vec![unknown_report, report])
        .with_commission(commission, Rc::new(Cell::new(false)));

    let events = ctx.manager.check_open_orders(&[&client]).await;
    for event in &events {
        ctx.exec_engine.borrow_mut().process(event);
    }

    let order = ctx.get_order(&client_order_id).unwrap();
    let cache = ctx.cache.borrow();

    assert_eq!(events.len(), 1);

    let OrderEventAny::Filled(fill) = &events[0] else {
        panic!("Expected fill for known order, was {:?}", events[0]);
    };

    assert_eq!(fill.client_order_id, client_order_id);
    assert_eq!(fill.venue_order_id, venue_order_id);
    assert_eq!(fill.last_qty, Quantity::from("4.5"));
    assert_eq!(fill.last_px, Price::from("101.25"));
    assert_eq!(fill.commission, Some(commission));
    assert_eq!(order.status(), OrderStatus::PartiallyFilled);
    assert_eq!(order.filled_qty(), Quantity::from("4.5"));
    assert_eq!(order.avg_px(), Some(dec!(101.25)));
    assert_eq!(
        order.commissions().get(&Currency::USDT()),
        Some(&commission)
    );
    assert_eq!(cache.orders(None, None, None, None, None).len(), 1);
    assert_eq!(cache.client_order_id(&unknown_venue_order_id), None);
    assert!(cache.order(&unknown_client_order_id).is_none());
    assert_eq!(client.order_report_query_count.get(), 0);
    assert_eq!(
        ctx.manager
            .recon_check_retry_count(&unknown_client_order_id),
        0
    );
}

#[rstest]
#[case::client_id(true, true)]
#[case::venue_id(true, false)]
#[case::instrument(false, true)]
#[tokio::test]
async fn test_check_open_orders_skips_excluded_reports(
    #[case] exclude_client_order: bool,
    #[case] has_client_id: bool,
) {
    let client_order_id = ClientOrderId::from("O-EXCLUDED-REPORT");
    let venue_order_id = VenueOrderId::from("V-EXCLUDED-REPORT");

    let mut config = ExecutionManagerConfig {
        open_check_open_only: false,
        open_check_missing_retries: 1,
        ..Default::default()
    };

    if exclude_client_order {
        config.filtered_client_order_ids.insert(client_order_id);
    } else {
        config
            .reconciliation_instrument_ids
            .insert(test_instrument_id2());
    }

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());
    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, test_client_id());
    let report = create_order_report(
        has_client_id.then_some(client_order_id),
        venue_order_id,
        test_instrument_id(),
        OrderStatus::PartiallyFilled,
        Quantity::from("10.0"),
        Quantity::from("4.5"),
    )
    .with_avg_px(dec!(101.25));
    let client = MockExecutionClient::new(vec![report]);

    let events = ctx.manager.check_open_orders(&[&client]).await;
    let order = ctx.get_order(&client_order_id).unwrap();

    assert!(events.is_empty());
    assert_eq!(order.status(), OrderStatus::Accepted);
    assert_eq!(order.filled_qty(), Quantity::from("0.0"));
    assert_eq!(client.order_report_query_count.get(), 0);
    assert_eq!(ctx.manager.recon_check_retry_count(&client_order_id), 0);
}

#[tokio::test]
async fn test_check_open_orders_skips_excluded_missing_order() {
    let client_order_id = ClientOrderId::from("O-EXCLUDED-MISSING");
    let venue_order_id = VenueOrderId::from("V-EXCLUDED-MISSING");

    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        filtered_client_order_ids: IndexSet::from([client_order_id]),
        open_check_open_only: false,
        open_check_missing_retries: 1,
        ..Default::default()
    });

    ctx.add_instrument(test_instrument());
    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, test_client_id());
    let client = MockExecutionClient::new(Vec::new());

    let events = ctx.manager.check_open_orders(&[&client]).await;

    assert!(events.is_empty());
    assert_eq!(client.order_report_query_count.get(), 0);
    assert_eq!(ctx.manager.recon_check_retry_count(&client_order_id), 0);
    assert_eq!(
        ctx.get_order(&client_order_id).unwrap().status(),
        OrderStatus::Accepted
    );
}

#[rstest]
#[tokio::test]
async fn test_check_open_orders_targeted_query_prevents_false_rejection() {
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 1,
        open_check_open_only: false,
        single_order_query_delay_ms: 0,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-TARGETED-FOUND");
    let venue_order_id = VenueOrderId::from("V-TARGETED-FOUND");
    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, test_client_id());

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        test_instrument_id(),
        OrderStatus::Accepted,
        Quantity::from("10.0"),
        Quantity::zero(1),
    )
    .with_price(Price::from("100.0"));
    let mock_client = MockExecutionClient::new(Vec::new()).with_order_report(report);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_open_orders(&clients).await;

    assert!(events.is_empty());
    assert_eq!(mock_client.order_report_query_count.get(), 1);
    assert_eq!(ctx.manager.recon_check_retry_count(&client_order_id), 0);
}

#[rstest]
#[tokio::test]
async fn test_check_open_orders_targeted_query_error_defers_resolution() {
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 1,
        open_check_open_only: false,
        single_order_query_delay_ms: 0,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-TARGETED-ERROR");
    let venue_order_id = VenueOrderId::from("V-TARGETED-ERROR");
    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, test_client_id());

    let mock_client = MockExecutionClient::new(Vec::new()).with_failed_order_report();
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_open_orders(&clients).await;

    assert!(events.is_empty());
    assert_eq!(mock_client.order_report_query_count.get(), 1);
    assert_eq!(ctx.manager.recon_check_retry_count(&client_order_id), 1);
}

#[rstest]
#[tokio::test]
async fn test_check_open_orders_mismatched_targeted_report_defers_resolution() {
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 1,
        open_check_open_only: false,
        single_order_query_delay_ms: 0,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-TARGETED-MISMATCH");
    let venue_order_id = VenueOrderId::from("V-TARGETED-MISMATCH");
    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, test_client_id());

    let report = create_order_report(
        Some(ClientOrderId::from("O-OTHER")),
        VenueOrderId::from("V-OTHER"),
        test_instrument_id(),
        OrderStatus::Accepted,
        Quantity::from("10.0"),
        Quantity::zero(1),
    )
    .with_price(Price::from("100.0"));
    let mock_client = MockExecutionClient::new(Vec::new()).with_order_report(report);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_open_orders(&clients).await;

    assert!(events.is_empty());
    assert_eq!(mock_client.order_report_query_count.get(), 1);
    assert_eq!(ctx.manager.recon_check_retry_count(&client_order_id), 1);
}

#[rstest]
#[tokio::test]
async fn test_check_open_orders_caps_targeted_queries_per_cycle(
    #[values(false, true)] open_only: bool,
) {
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 1,
        open_check_open_only: open_only,
        max_single_order_queries_per_cycle: 1,
        single_order_query_delay_ms: 0,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    let client_a = ClientId::from("CLIENT-A");
    let client_b = ClientId::from("CLIENT-B");
    insert_accepted_limit_order(
        &ctx,
        ClientOrderId::from("O-TARGETED-A"),
        VenueOrderId::from("V-TARGETED-A"),
        client_a,
    );
    insert_accepted_limit_order(
        &ctx,
        ClientOrderId::from("O-TARGETED-B"),
        VenueOrderId::from("V-TARGETED-B"),
        client_b,
    );

    let mock_a = MockExecutionClient::for_venue(client_a, test_venue(), Vec::new());
    let mock_b = MockExecutionClient::for_venue(client_b, test_venue(), Vec::new());
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_a, &mock_b];

    let events = ctx.manager.check_open_orders(&clients).await;

    assert_eq!(events.len(), usize::from(!open_only));
    assert_eq!(mock_a.order_report_query_count.get(), 1);
    assert_eq!(mock_b.order_report_query_count.get(), 0);
}

#[rstest]
#[tokio::test]
async fn test_check_open_orders_queries_oversized_responsible_client_group() {
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 1,
        open_check_open_only: false,
        max_single_order_queries_per_cycle: 1,
        single_order_query_delay_ms: 0,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-TARGETED-MULTI");
    let venue_order_id = VenueOrderId::from("V-TARGETED-MULTI");
    let order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .instrument_id(test_instrument_id())
        .quantity(Quantity::from("10.0"))
        .price(Price::from("100.0"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    ctx.cache
        .borrow_mut()
        .add_order(order, None, None, false)
        .unwrap();
    let order = ctx.cache.borrow_mut().update_order(&submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    ctx.cache.borrow_mut().update_order(&accepted).unwrap();

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        test_instrument_id(),
        OrderStatus::Accepted,
        Quantity::from("10.0"),
        Quantity::zero(1),
    )
    .with_price(Price::from("100.0"));
    let mock_a =
        MockExecutionClient::for_venue(ClientId::from("CLIENT-A"), test_venue(), Vec::new());
    let mock_b =
        MockExecutionClient::for_venue(ClientId::from("CLIENT-B"), test_venue(), Vec::new())
            .with_order_report(report);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_a, &mock_b];

    let events = ctx.manager.check_open_orders(&clients).await;

    assert!(events.is_empty());
    assert_eq!(
        (
            mock_a.order_report_query_count.get(),
            mock_b.order_report_query_count.get(),
            ctx.manager.recon_check_retry_count(&client_order_id),
        ),
        (1, 1, 0),
    );
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_check_open_orders_spaces_targeted_queries() {
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 1,
        open_check_open_only: false,
        max_single_order_queries_per_cycle: 2,
        single_order_query_delay_ms: 100,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    let client_a = ClientId::from("CLIENT-A");
    let client_b = ClientId::from("CLIENT-B");
    insert_accepted_limit_order(
        &ctx,
        ClientOrderId::from("O-TARGETED-A"),
        VenueOrderId::from("V-TARGETED-A"),
        client_a,
    );
    insert_accepted_limit_order(
        &ctx,
        ClientOrderId::from("O-TARGETED-B"),
        VenueOrderId::from("V-TARGETED-B"),
        client_b,
    );

    let query_times = Rc::new(RefCell::new(Vec::new()));

    let mock_a = MockExecutionClient::for_venue(client_a, test_venue(), Vec::new())
        .with_on_order_report_query(Box::new({
            let query_times = query_times.clone();
            move || query_times.borrow_mut().push(dst::time::Instant::now())
        }));

    let mock_b = MockExecutionClient::for_venue(client_b, test_venue(), Vec::new())
        .with_on_order_report_query(Box::new({
            let query_times = query_times.clone();
            move || query_times.borrow_mut().push(dst::time::Instant::now())
        }));

    let clients: Vec<&dyn ExecutionClient> = vec![&mock_a, &mock_b];

    let events = ctx.manager.check_open_orders(&clients).await;
    let query_times = query_times.borrow();

    assert_eq!(events.len(), 2);
    assert_eq!(query_times.len(), 2);
    assert!(query_times[1].duration_since(query_times[0]) >= dst::time::Duration::from_millis(100));
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_check_open_orders_partially_filled_missing_at_venue_generates_canceled() {
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 1,
        open_check_open_only: false,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    ctx.add_instrument(instrument.clone());

    let client_order_id = ClientOrderId::from("O-PARTIAL-MISSING");
    let venue_order_id = VenueOrderId::from("V-PARTIAL-MISSING");
    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, test_client_id());
    let order = ctx.get_order(&client_order_id).unwrap();
    let fill = OrderFilledTestBuilder::new(&order, &instrument)
        .last_qty(Quantity::from("4.0"))
        .without_position_id()
        .build();
    ctx.cache.borrow_mut().update_order(&fill).unwrap();

    let mock_client = MockExecutionClient::new(vec![]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_open_orders(&clients).await;

    assert_eq!(events.len(), 1);
    assert!(
        matches!(&events[0], OrderEventAny::Canceled(canceled) if canceled.client_order_id == client_order_id),
        "expected OrderCanceled event, was {:?}",
        events[0],
    );

    ctx.cache.borrow_mut().update_order(&events[0]).unwrap();
    let order = ctx.get_order(&client_order_id).unwrap();
    assert_eq!(order.status(), OrderStatus::Canceled);
    assert_eq!(order.filled_qty(), Quantity::from("4.0"));
}

#[rstest]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_check_open_orders_failed_client_does_not_advance_missing_retries(
    #[values(false, true)] open_only: bool,
) {
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 2,
        open_check_open_only: open_only,
        single_order_query_delay_ms: 0,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());
    ctx.add_instrument(test_instrument2());

    let healthy_order_id = ClientOrderId::from("O-HEALTHY-MISSING");
    let failed_order_id = ClientOrderId::from("O-FAILED-VENUE");
    let healthy_client_id = test_client_id();
    let failed_client_id = ClientId::from("BYBIT");
    let failed_venue = Venue::from("BYBIT");

    insert_accepted_limit_order(
        &ctx,
        healthy_order_id,
        VenueOrderId::from("V-HEALTHY-MISSING"),
        healthy_client_id,
    );
    insert_accepted_limit_order_for_instrument(
        &ctx,
        failed_order_id,
        VenueOrderId::from("V-FAILED-VENUE"),
        test_instrument_id2(),
        failed_client_id,
    );

    let healthy_client = MockExecutionClient::for_venue(healthy_client_id, test_venue(), vec![]);
    let failed_client = MockExecutionClient::failing(failed_client_id, failed_venue);
    let clients: Vec<&dyn ExecutionClient> = vec![&healthy_client, &failed_client];

    let first_events = ctx.manager.check_open_orders(&clients).await;

    assert!(first_events.is_empty());

    advance_clock(dst::time::Duration::from_millis(1)).await;

    let recovered_client = MockExecutionClient::for_venue(failed_client_id, failed_venue, vec![]);
    let clients: Vec<&dyn ExecutionClient> = vec![&healthy_client, &recovered_client];
    let second_events = ctx.manager.check_open_orders(&clients).await;

    assert_eq!(healthy_client.order_report_query_count.get(), 1);
    assert_eq!(recovered_client.order_report_query_count.get(), 0);

    if open_only {
        assert!(second_events.is_empty());
    } else {
        assert_eq!(second_events.len(), 1);
        assert!(
            matches!(&second_events[0], OrderEventAny::Rejected(rejected)
            if rejected.client_order_id == healthy_order_id)
        );
    }

    assert_eq!(
        ctx.get_order(&failed_order_id).unwrap().status(),
        OrderStatus::Accepted,
    );
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_check_open_orders_failed_routing_client_does_not_resolve_exchange_order() {
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 1,
        open_check_open_only: false,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let client_order_id = ClientOrderId::from("O-FAILED-ROUTING");
    let routing_client_id = ClientId::from("IB");
    ctx.add_instrument(instrument);
    insert_accepted_limit_order(
        &ctx,
        client_order_id,
        VenueOrderId::from("V-FAILED-ROUTING"),
        routing_client_id,
    );

    let routing_client = MockExecutionClient::failing(routing_client_id, Venue::from("IB"))
        .with_account_id(AccountId::from("IB-001"))
        .with_handled_venues(IndexSet::from([instrument_id.venue]));
    let clients: Vec<&dyn ExecutionClient> = vec![&routing_client];

    let events = ctx.manager.check_open_orders(&clients).await;

    assert!(events.is_empty());
    assert_eq!(
        ctx.get_order(&client_order_id).unwrap().status(),
        OrderStatus::Accepted,
    );
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_check_open_orders_failed_client_does_not_suppress_healthy_client_same_venue() {
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 1,
        open_check_open_only: false,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    let failed_order_id = ClientOrderId::from("O-FAILED-SHARED-VENUE");
    let healthy_order_id = ClientOrderId::from("O-HEALTHY-SHARED-VENUE");
    let failed_client_id = ClientId::from("SHARED-FAILED");
    let healthy_client_id = ClientId::from("SHARED-HEALTHY");
    insert_accepted_limit_order(
        &ctx,
        failed_order_id,
        VenueOrderId::from("V-FAILED-SHARED-VENUE"),
        failed_client_id,
    );
    insert_accepted_limit_order(
        &ctx,
        healthy_order_id,
        VenueOrderId::from("V-HEALTHY-SHARED-VENUE"),
        healthy_client_id,
    );

    let failed_client = MockExecutionClient::failing(failed_client_id, test_venue());
    let healthy_client = MockExecutionClient::for_venue(healthy_client_id, test_venue(), vec![]);
    let clients: Vec<&dyn ExecutionClient> = vec![&failed_client, &healthy_client];

    let events = ctx.manager.check_open_orders(&clients).await;

    assert_eq!(events.len(), 1);
    assert!(
        matches!(&events[0], OrderEventAny::Rejected(rejected) if rejected.client_order_id == healthy_order_id),
        "only the healthy-client order should be rejected",
    );
    assert_eq!(
        ctx.get_order(&failed_order_id).unwrap().status(),
        OrderStatus::Accepted,
    );
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_check_open_orders_failed_routing_client_fallback_coverage_does_not_advance() {
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 1,
        open_check_open_only: false,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let client_order_id = ClientOrderId::from("O-FAILED-ROUTING-FALLBACK");
    ctx.add_instrument(instrument);

    let order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(client_order_id)
        .instrument_id(instrument_id)
        .quantity(Quantity::from("10.0"))
        .price(Price::from("100.0"))
        .build();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    ctx.add_order(order);
    let order = ctx.cache.borrow_mut().update_order(&submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(
        &order,
        test_account_id(),
        VenueOrderId::from("V-FAILED-ROUTING-FALLBACK"),
    );
    ctx.cache.borrow_mut().update_order(&accepted).unwrap();

    let routing_client = MockExecutionClient::failing(ClientId::from("IB"), Venue::from("IB"))
        .with_account_id(AccountId::from("IB-001"))
        .with_handled_venues(IndexSet::from([instrument_id.venue]));
    let clients: Vec<&dyn ExecutionClient> = vec![&routing_client];

    let events = ctx.manager.check_open_orders(&clients).await;

    assert!(events.is_empty());
    assert_eq!(
        ctx.get_order(&client_order_id).unwrap().status(),
        OrderStatus::Accepted,
    );
}

#[rstest]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_check_open_orders_positive_report_resets_missing_retry_ladder(
    #[values(false, true)] open_only: bool,
) {
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 2,
        open_check_open_only: open_only,
        single_order_query_delay_ms: 0,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-RETRY-RESET");
    let venue_order_id = VenueOrderId::from("V-RETRY-RESET");
    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, test_client_id());

    let empty_client = MockExecutionClient::new(vec![]);
    let clients: Vec<&dyn ExecutionClient> = vec![&empty_client];
    let first_events = ctx.manager.check_open_orders(&clients).await;

    assert!(first_events.is_empty());
    assert_eq!(ctx.manager.recon_check_retry_count(&client_order_id), 1);

    advance_clock(dst::time::Duration::from_millis(1)).await;

    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        test_instrument_id(),
        OrderStatus::Accepted,
        Quantity::from("10.0"),
        Quantity::from("0.0"),
    );
    let reporting_client = MockExecutionClient::new(vec![report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&reporting_client];
    let second_events = ctx.manager.check_open_orders(&clients).await;

    assert!(
        !second_events
            .iter()
            .any(|e| matches!(e, OrderEventAny::Rejected(_) | OrderEventAny::Canceled(_))),
        "a positive matching report must not resolve the order",
    );
    assert_eq!(ctx.manager.recon_check_retry_count(&client_order_id), 0);

    advance_clock(dst::time::Duration::from_millis(1)).await;

    let empty_again = MockExecutionClient::new(vec![]);
    let clients: Vec<&dyn ExecutionClient> = vec![&empty_again];
    let third_events = ctx.manager.check_open_orders(&clients).await;

    assert!(
        third_events.is_empty(),
        "non-consecutive misses must not exhaust the retry budget",
    );
    assert_eq!(ctx.manager.recon_check_retry_count(&client_order_id), 1);
    assert_eq!(
        ctx.get_order(&client_order_id).unwrap().status(),
        OrderStatus::Accepted,
    );
    assert_eq!(empty_again.order_report_query_count.get(), 0);
    let fourth_events = ctx.manager.check_open_orders(&clients).await;
    assert_eq!(empty_again.order_report_query_count.get(), 1);
    assert_eq!(fourth_events.len(), usize::from(!open_only));
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_check_open_orders_venue_id_only_report_resets_missing_retry_ladder() {
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 2,
        open_check_open_only: false,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-VENUE-ID-ONLY");
    let venue_order_id = VenueOrderId::from("V-VENUE-ID-ONLY");
    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, test_client_id());

    let empty_client = MockExecutionClient::new(vec![]);
    let clients: Vec<&dyn ExecutionClient> = vec![&empty_client];
    let first_events = ctx.manager.check_open_orders(&clients).await;

    assert!(first_events.is_empty());
    assert_eq!(ctx.manager.recon_check_retry_count(&client_order_id), 1);

    advance_clock(dst::time::Duration::from_millis(1)).await;

    // The report carries no client order ID; the manager must map it through
    // the cached venue-order-ID index and give it full positive-report
    // bookkeeping, not walk it as missing in the same pass.
    let report = create_order_report(
        None,
        venue_order_id,
        test_instrument_id(),
        OrderStatus::Accepted,
        Quantity::from("10.0"),
        Quantity::from("0.0"),
    );
    let reporting_client = MockExecutionClient::new(vec![report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&reporting_client];
    let second_events = ctx.manager.check_open_orders(&clients).await;

    assert!(
        !second_events
            .iter()
            .any(|e| matches!(e, OrderEventAny::Rejected(_) | OrderEventAny::Canceled(_))),
        "a venue-ID-only positive report must not resolve the order",
    );
    assert_eq!(ctx.manager.recon_check_retry_count(&client_order_id), 0);

    advance_clock(dst::time::Duration::from_millis(1)).await;

    let empty_again = MockExecutionClient::new(vec![]);
    let clients: Vec<&dyn ExecutionClient> = vec![&empty_again];
    let third_events = ctx.manager.check_open_orders(&clients).await;

    assert!(
        third_events.is_empty(),
        "a single miss after a venue-ID-only positive report must not reject",
    );
    assert_eq!(ctx.manager.recon_check_retry_count(&client_order_id), 1);
    assert_eq!(
        ctx.get_order(&client_order_id).unwrap().status(),
        OrderStatus::Accepted,
    );
}

#[rstest]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_check_open_orders_order_closed_during_query_leaves_no_retry_state(
    #[values(false, true)] open_only: bool,
) {
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 2,
        open_check_open_only: open_only,
        single_order_query_delay_ms: 0,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-CLOSED-DURING-QUERY");
    let venue_order_id = VenueOrderId::from("V-CLOSED-DURING-QUERY");
    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, test_client_id());

    // The callback fires inside the report query, after the prepare-time
    // snapshot captured the order as an open missing-candidate.
    let cache = ctx.cache.clone();
    let client =
        MockExecutionClient::new(vec![]).with_on_order_reports_query(Box::new(move || {
            let order = cache.borrow().order(&client_order_id).unwrap().clone();
            let canceled =
                TestOrderEventStubs::canceled(&order, test_account_id(), Some(venue_order_id));
            cache.borrow_mut().update_order(&canceled).unwrap();
        }));

    let clients: Vec<&dyn ExecutionClient> = vec![&client];

    let events = ctx.manager.check_open_orders(&clients).await;

    assert!(events.is_empty());
    assert_eq!(client.order_report_query_count.get(), 0);
    assert_eq!(
        ctx.manager.recon_check_retry_count(&client_order_id),
        0,
        "an order that closed during the query must leave no retry state behind",
    );
    assert_eq!(
        ctx.get_order(&client_order_id).unwrap().status(),
        OrderStatus::Canceled,
    );
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_check_open_orders_deferred_pending_order_keeps_inflight_registration() {
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 1,
        open_check_open_only: false,
        inflight_threshold_ms: 100,
        inflight_max_retries: 2,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    let client_order_id = ClientOrderId::from("O-PENDING-DEFER");
    let venue_order_id = VenueOrderId::from("V-PENDING-DEFER");
    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, test_client_id());
    let order = ctx.get_order(&client_order_id).unwrap();
    let pending = OrderEventAny::PendingCancel(
        OrderPendingCancelSpec::builder()
            .trader_id(order.trader_id())
            .strategy_id(order.strategy_id())
            .instrument_id(order.instrument_id())
            .client_order_id(client_order_id)
            .account_id(test_account_id())
            .venue_order_id(venue_order_id)
            .build(),
    );
    ctx.cache.borrow_mut().update_order(&pending).unwrap();
    assert_eq!(
        ctx.get_order(&client_order_id).unwrap().status(),
        OrderStatus::PendingCancel,
    );
    ctx.manager.register_inflight(client_order_id, UUID4::new());

    // Prime one inflight retry so the retained check carries a nonzero count
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;
    let primed = ctx.manager.check_inflight_orders();
    assert!(primed.events.is_empty());
    assert_eq!(primed.queries.len(), 1);

    // Missing at venue at max retries: the inflight status defers resolution
    let mock_client = MockExecutionClient::new(vec![]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];
    let events = ctx.manager.check_open_orders(&clients).await;
    assert!(events.is_empty());

    // The deferral must not unregister the order from inflight recovery, and
    // must reset the inflight retry ladder: past the threshold the checker
    // queries again from scratch instead of escalating to a stale-count
    // cancellation
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;
    let result = ctx.manager.check_inflight_orders();
    assert!(
        result.events.is_empty(),
        "deferral must reset the inflight retry ladder, was {:?}",
        result.events,
    );
    assert_eq!(
        result.queries.len(),
        1,
        "deferred pending order should remain registered for inflight recovery",
    );
}

#[rstest]
#[tokio::test]
async fn test_check_open_orders_open_only_negative_query_keeps_order_open(
    #[values(false, true)] failed: bool,
    #[values(false, true)] partially_filled: bool,
) {
    let _guard = MANAGER_LOG_TEST_LOCK.lock().await;
    install_manager_log_capture();

    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 2,
        single_order_query_delay_ms: 0,
        ..Default::default()
    });

    let instrument = test_instrument();
    ctx.add_instrument(instrument.clone());
    let client_order_id = ClientOrderId::from("O-OPEN-NEGATIVE");
    let venue_order_id = VenueOrderId::from("V-OPEN-NEGATIVE");
    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, test_client_id());
    if partially_filled {
        let order = ctx.get_order(&client_order_id).unwrap();
        let fill = OrderFilledTestBuilder::new(&order, &instrument)
            .last_qty(Quantity::from("4.0"))
            .without_position_id()
            .build();
        ctx.cache.borrow_mut().update_order(&fill).unwrap();
    }

    let before = ctx.get_order(&client_order_id).unwrap();
    let mut client = MockExecutionClient::new(Vec::new());
    client.fail_order_report = failed;
    for expected_queries in [0, 1, 1, 2] {
        let events = ctx.manager.check_open_orders(&[&client]).await;
        assert!(events.is_empty());
        assert_eq!(client.order_report_query_count.get(), expected_queries);
        assert_eq!(ctx.get_order(&client_order_id).unwrap(), before);
    }

    let warnings = MANAGER_LOG_CAPTURE.messages.lock();
    let warnings: Vec<_> = warnings
        .iter()
        .filter(|message| message.contains(client_order_id.as_str()))
        .collect();
    assert_eq!(warnings.len(), 2, "found {warnings:?}");

    let expected = if failed {
        "Failed targeted order status query"
    } else {
        "Keeping O-OPEN-NEGATIVE open: targeted order status query found no report"
    };

    assert!(warnings.iter().all(|message| message.starts_with(expected)));
    assert!(
        ctx.manager
            .take_submission_recovery_exhaustions()
            .is_empty()
    );
}

#[rstest]
#[tokio::test]
async fn test_check_open_orders_bulk_failure_warning_rearms_after_recovery(
    #[values(false, true)] open_only: bool,
) {
    let _guard = MANAGER_LOG_TEST_LOCK.lock().await;
    install_manager_log_capture();

    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        open_check_open_only: open_only,
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 2,
        single_order_query_delay_ms: 0,
        ..Default::default()
    });

    ctx.add_instrument(test_instrument());
    let client_order_id = ClientOrderId::from("O-BULK-FAILURE-WARN");
    insert_accepted_limit_order(
        &ctx,
        client_order_id,
        VenueOrderId::from("V-BULK-FAILURE-WARN"),
        test_client_id(),
    );
    let before = ctx.get_order(&client_order_id).unwrap();
    let mut client = MockExecutionClient::failing(test_client_id(), test_venue());

    let attempts = 3;
    for _ in 0..attempts {
        assert!(ctx.manager.check_open_orders(&[&client]).await.is_empty());
    }

    let warnings = MANAGER_LOG_CAPTURE
        .messages
        .lock()
        .iter()
        .filter(|message| message.contains(client_order_id.as_str()))
        .count();
    assert_eq!(warnings, if open_only { 1 } else { attempts });

    client.fail_order_reports = false;
    assert!(ctx.manager.check_open_orders(&[&client]).await.is_empty());
    client.fail_order_reports = true;
    assert!(ctx.manager.check_open_orders(&[&client]).await.is_empty());

    let warnings = MANAGER_LOG_CAPTURE
        .messages
        .lock()
        .iter()
        .filter(|message| message.contains(client_order_id.as_str()))
        .count();
    assert_eq!(warnings, if open_only { 2 } else { attempts + 1 });
    assert_eq!(ctx.get_order(&client_order_id).unwrap(), before);
    assert_eq!(client.order_report_query_count.get(), 0);
    assert_eq!(ctx.manager.recon_check_retry_count(&client_order_id), 1);
}

#[tokio::test]
async fn test_check_open_orders_open_only_does_not_reopen_order_closed_during_targeted_query() {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 1,
        single_order_query_delay_ms: 0,
        ..Default::default()
    });

    ctx.add_instrument(test_instrument());
    let client_order_id = ClientOrderId::from("O-CLOSED-DURING-TARGET");
    let venue_order_id = VenueOrderId::from("V-CLOSED-DURING-TARGET");
    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, test_client_id());
    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        test_instrument_id(),
        OrderStatus::Accepted,
        Quantity::from("10.0"),
        Quantity::zero(1),
    );
    let mut expected = ctx.get_order(&client_order_id).unwrap();
    let canceled =
        TestOrderEventStubs::canceled(&expected, test_account_id(), Some(venue_order_id));
    expected.apply(canceled.clone()).unwrap();
    let cache = ctx.cache.clone();
    let client = MockExecutionClient::new(Vec::new())
        .with_order_report(report)
        .with_on_order_report_query(Box::new(move || {
            cache.borrow_mut().update_order(&canceled).unwrap();
        }));

    let events = ctx.manager.check_open_orders(&[&client]).await;

    assert!(events.is_empty());
    assert_eq!(client.order_report_query_count.get(), 1);
    assert_eq!(ctx.get_order(&client_order_id).unwrap(), expected);
    assert_eq!(ctx.manager.recon_check_retry_count(&client_order_id), 0);
}

#[rstest]
#[tokio::test]
async fn test_check_open_orders_missing_lookback_skips_query(
    #[values(false, true)] open_only: bool,
) {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 1,
        open_check_open_only: open_only,
        open_check_lookback_mins: Some(1),
        ..Default::default()
    });

    ctx.add_instrument(test_instrument());
    let client_order_id = ClientOrderId::from("O-LOOKBACK-MISSING");
    insert_accepted_limit_order(
        &ctx,
        client_order_id,
        VenueOrderId::from("V-LOOKBACK-MISSING"),
        test_client_id(),
    );
    let before = ctx.get_order(&client_order_id).unwrap();
    ctx.advance_time(DurationNanos::from_mins(2).as_u64());
    let client = MockExecutionClient::new(Vec::new());

    let events = ctx.manager.check_open_orders(&[&client]).await;

    assert!(events.is_empty());
    assert_eq!(client.order_report_query_count.get(), 0);
    assert_eq!(ctx.get_order(&client_order_id).unwrap(), before);
}

#[rstest]
#[case::cancel(false)]
#[case::modify(true)]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_check_open_orders_open_only_queries_after_inflight_timeout(#[case] modify: bool) {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 2,
        inflight_threshold_ms: 10,
        inflight_max_retries: 2,
        single_order_query_delay_ms: 0,
        ..Default::default()
    });

    ctx.add_instrument(test_instrument());
    let client_order_id = ClientOrderId::from("O-OPEN-RESTORED");
    let venue_order_id = VenueOrderId::from("V-OPEN-RESTORED");
    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, test_client_id());
    let order = ctx.get_order(&client_order_id).unwrap();

    let pending = if modify {
        OrderEventAny::PendingUpdate(
            OrderPendingUpdateSpec::builder()
                .trader_id(order.trader_id())
                .strategy_id(order.strategy_id())
                .instrument_id(order.instrument_id())
                .client_order_id(client_order_id)
                .venue_order_id(venue_order_id)
                .account_id(test_account_id())
                .build(),
        )
    } else {
        OrderEventAny::PendingCancel(
            OrderPendingCancelSpec::builder()
                .trader_id(order.trader_id())
                .strategy_id(order.strategy_id())
                .instrument_id(order.instrument_id())
                .client_order_id(client_order_id)
                .venue_order_id(venue_order_id)
                .account_id(test_account_id())
                .build(),
        )
    };

    ctx.manager.register_inflight(client_order_id, UUID4::new());
    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        test_instrument_id(),
        OrderStatus::Canceled,
        Quantity::from("10.0"),
        Quantity::zero(1),
    );
    let client = MockExecutionClient::new(Vec::new()).with_order_report(report);

    for _ in 0..2 {
        assert!(ctx.manager.check_open_orders(&[&client]).await.is_empty());
    }

    assert_eq!(client.order_report_query_count.get(), 0);
    assert_eq!(ctx.manager.recon_check_retry_count(&client_order_id), 0);
    ctx.cache.borrow_mut().update_order(&pending).unwrap();

    ctx.advance_both(dst::time::Duration::from_millis(20)).await;
    let inflight = ctx.manager.check_inflight_orders();
    assert_eq!(inflight.queries.len(), 1);
    assert!(inflight.events.is_empty());
    assert!(ctx.manager.check_open_orders(&[&client]).await.is_empty());
    assert_eq!(client.order_report_query_count.get(), 0);
    assert_eq!(ctx.manager.recon_check_retry_count(&client_order_id), 1);

    ctx.advance_both(dst::time::Duration::from_millis(20)).await;
    let exhausted = ctx.manager.check_inflight_orders();
    assert!(exhausted.queries.is_empty());
    assert_eq!(exhausted.events.len(), 1);

    match &exhausted.events[0] {
        OrderEventAny::ModifyRejected(event) if modify => {
            assert_eq!(event.reason, "INFLIGHT_TIMEOUT");
            assert_eq!(event.client_order_id, client_order_id);
        }
        OrderEventAny::CancelRejected(event) if !modify => {
            assert_eq!(event.reason, "INFLIGHT_TIMEOUT");
            assert_eq!(event.client_order_id, client_order_id);
        }
        event => panic!("Expected command rejection, was {event:?}"),
    }

    ctx.exec_engine.borrow_mut().process(&exhausted.events[0]);
    ctx.manager.observe_order_event(&exhausted.events[0]);
    assert_eq!(
        ctx.get_order(&client_order_id).unwrap().status(),
        OrderStatus::Accepted
    );
    assert_eq!(ctx.manager.recon_check_retry_count(&client_order_id), 0);

    assert!(ctx.manager.check_open_orders(&[&client]).await.is_empty());
    assert_eq!(client.order_report_query_count.get(), 0);
    let events = ctx.manager.check_open_orders(&[&client]).await;
    assert_eq!(client.order_report_query_count.get(), 1);
    assert_eq!(events.len(), 1);
    assert!(matches!(&events[0], OrderEventAny::Canceled(event)
        if event.client_order_id == client_order_id && event.venue_order_id == Some(venue_order_id)));
    ctx.exec_engine.borrow_mut().process(&events[0]);
    assert_eq!(
        ctx.get_order(&client_order_id).unwrap().status(),
        OrderStatus::Canceled
    );
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_check_open_orders_open_only_defers_recent_queries_and_activity() {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::from_millis(100),
        open_check_missing_retries: 1,
        single_order_query_delay_ms: 200,
        ..Default::default()
    });

    ctx.add_instrument(test_instrument());
    let client_order_id = ClientOrderId::from("O-OPEN-RECENT");
    insert_accepted_limit_order(
        &ctx,
        client_order_id,
        VenueOrderId::from("V-OPEN-RECENT"),
        test_client_id(),
    );
    ctx.manager.record_local_activity(client_order_id);
    let client = MockExecutionClient::new(Vec::new());

    assert!(ctx.manager.check_open_orders(&[&client]).await.is_empty());
    assert_eq!(client.order_report_query_count.get(), 0);
    ctx.advance_both(dst::time::Duration::from_millis(101))
        .await;
    assert!(ctx.manager.check_open_orders(&[&client]).await.is_empty());
    assert_eq!(client.order_report_query_count.get(), 1);
    ctx.advance_both(dst::time::Duration::from_millis(199))
        .await;
    assert!(ctx.manager.check_open_orders(&[&client]).await.is_empty());
    assert_eq!(client.order_report_query_count.get(), 1);
    ctx.advance_both(dst::time::Duration::from_millis(1)).await;
    assert!(ctx.manager.check_open_orders(&[&client]).await.is_empty());
    assert_eq!(client.order_report_query_count.get(), 2);
}

#[rstest]
#[tokio::test]
async fn test_check_open_orders_open_only_missing_venue_order_does_not_reject() {
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 1,
        open_check_open_only: true,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    let order = create_limit_order(
        "O-OPEN-ONLY",
        test_instrument_id(),
        OrderSide::Buy,
        "10.0",
        "100.0",
    );
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    ctx.add_order(order);
    ctx.cache.borrow_mut().update_order(&submitted).unwrap();

    let mock_client = MockExecutionClient::new(vec![]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_open_orders(&clients).await;

    assert!(events.is_empty());
    assert_eq!(mock_client.order_report_query_count.get(), 0);
    let cached_order = ctx.get_order(&ClientOrderId::from("O-OPEN-ONLY")).unwrap();
    assert_eq!(cached_order.status(), OrderStatus::Submitted);
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_check_open_orders_missing_gate_uses_local_activity_not_venue_ts_last() {
    // A corrupted far-future ts_last must not stall missing-order reconciliation
    // after the local activity grace expires.
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::from_millis(200),
        open_check_missing_retries: 1,
        open_check_open_only: false,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    let order = create_limit_order(
        "O-AHEAD",
        test_instrument_id(),
        OrderSide::Buy,
        "10.0",
        "100.0",
    );
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    ctx.add_order(order);
    let order = ctx.cache.borrow_mut().update_order(&submitted).unwrap();

    let future_ts = ctx
        .clock
        .borrow()
        .timestamp_ns()
        .saturating_add(DurationNanos::from_secs(10));
    let accepted = OrderEventAny::Accepted(
        OrderAcceptedSpec::builder()
            .trader_id(order.trader_id())
            .strategy_id(order.strategy_id())
            .instrument_id(order.instrument_id())
            .client_order_id(order.client_order_id())
            .venue_order_id(VenueOrderId::from("V-AHEAD"))
            .account_id(test_account_id())
            .ts_event(future_ts)
            .ts_init(future_ts)
            .build(),
    );
    let client_order_id = order.client_order_id();
    ctx.cache.borrow_mut().update_order(&accepted).unwrap();
    // Track the event exactly as the LiveNode dispatch path does: ack cleanup
    // plus the local-activity stamp, in that order.
    ctx.manager.observe_order_event(&accepted);

    let mock_client = MockExecutionClient::new(vec![]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_open_orders(&clients).await;

    assert!(
        events.is_empty(),
        "recent local activity should defer reconciliation",
    );

    advance_clock(dst::time::Duration::from_millis(250)).await;

    let events = ctx.manager.check_open_orders(&clients).await;

    assert_eq!(events.len(), 1);
    assert!(
        matches!(&events[0], OrderEventAny::Rejected(rejected) if rejected.client_order_id == client_order_id),
        "far-future venue ts_last must not keep deferring reconciliation",
    );
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_check_open_orders_defers_for_just_accepted_order() {
    // Regression: the LiveNode dispatch path for Accepted performs ack cleanup
    // (clear_recon_tracking) and stamps local activity via observe_order_event.
    // The stamp must survive the cleanup so a just-accepted order that a
    // lagging venue report omits defers until the grace expires, rather than
    // being rejected as missing.
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::from_millis(200),
        open_check_missing_retries: 1,
        open_check_open_only: false,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    let order = create_limit_order(
        "O-ACCEPTED",
        test_instrument_id(),
        OrderSide::Buy,
        "10.0",
        "100.0",
    );
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    ctx.add_order(order);
    let order = ctx.cache.borrow_mut().update_order(&submitted).unwrap();
    let client_order_id = order.client_order_id();

    let accepted =
        TestOrderEventStubs::accepted(&order, test_account_id(), VenueOrderId::from("V-ACCEPTED"));
    ctx.cache.borrow_mut().update_order(&accepted).unwrap();
    ctx.manager.observe_order_event(&accepted);

    // Venue response lags and omits the just-accepted order
    let mock_client = MockExecutionClient::new(vec![]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_open_orders(&clients).await;

    assert!(
        events.is_empty(),
        "a just-accepted order must defer missing-order reconciliation",
    );

    advance_clock(dst::time::Duration::from_millis(250)).await;

    let events = ctx.manager.check_open_orders(&clients).await;

    assert_eq!(events.len(), 1);
    assert!(
        matches!(&events[0], OrderEventAny::Rejected(rejected) if rejected.client_order_id == client_order_id),
        "reconciliation must proceed once the local-activity grace expires",
    );
}

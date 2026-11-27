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

use std::fmt::Debug;

use super::*;

#[rstest]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_canonical_duplicate_reconciliation_fill_is_committed() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-DUPLICATE-COMMIT");
    let venue_order_id = VenueOrderId::from("V-DUPLICATE-COMMIT");
    let retry_client_order_id = ClientOrderId::from("O-DUPLICATE-COMMIT-2");
    let retry_venue_order_id = VenueOrderId::from("V-DUPLICATE-COMMIT-2");
    let trade_id = TradeId::from("T-DUPLICATE-COMMIT");

    let instrument = test_instrument();
    ctx.add_instrument(instrument.clone());
    let mut order = create_accepted_order(
        client_order_id.as_str(),
        instrument_id,
        OrderSide::Buy,
        "2.0",
        "3000.00",
        venue_order_id,
    );
    let existing_fill = TestOrderEventStubs::filled(
        &order,
        &instrument,
        Some(trade_id),
        None,
        Some(Price::from("3000.00")),
        Some(Quantity::from("1.0")),
        Some(LiquiditySide::Maker),
        None,
        None,
        Some(test_account_id()),
    );
    order.apply(existing_fill).unwrap();
    ctx.add_order(order);

    let duplicate_report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("2.0"),
        Quantity::from("2.0"),
    );
    let duplicate_fill = create_fill_report(
        client_order_id,
        venue_order_id,
        instrument_id,
        trade_id,
        "1.0",
    );
    let duplicate = ctx.manager.reconcile_execution_mass_status(
        &create_mass_status(vec![duplicate_report], vec![duplicate_fill]),
        &ctx.exec_engine,
    );

    let [OrderEventAny::Filled(residual)] = duplicate.events.as_slice() else {
        panic!(
            "Expected one residual fill, received {:?}",
            duplicate.events
        );
    };

    assert_eq!(residual.client_order_id, client_order_id);
    assert_eq!(residual.last_qty, Quantity::from("1.0"));
    assert_ne!(residual.trade_id, trade_id);
    let reconciled = ctx.get_order(&client_order_id).unwrap();
    assert_eq!(reconciled.status(), OrderStatus::Filled);
    assert_eq!(reconciled.filled_qty(), Quantity::from("2.0"));

    ctx.add_order(create_accepted_order(
        retry_client_order_id.as_str(),
        instrument_id,
        OrderSide::Buy,
        "1.0",
        "3000.00",
        retry_venue_order_id,
    ));
    let retry_fill = create_fill_report(
        retry_client_order_id,
        retry_venue_order_id,
        instrument_id,
        trade_id,
        "1.0",
    );
    let retry = ctx.manager.reconcile_execution_mass_status(
        &create_mass_status(vec![], vec![retry_fill]),
        &ctx.exec_engine,
    );

    assert!(retry.events.is_empty());
    assert!(
        ctx.get_order(&retry_client_order_id)
            .unwrap()
            .trade_ids()
            .is_empty()
    );
}

#[rstest]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_rejected_reconciliation_fill_is_retried() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-REJECT-RETRY");
    let venue_order_id = VenueOrderId::from("V-REJECT-RETRY");
    let retry_client_order_id = ClientOrderId::from("O-REJECT-RETRY-2");
    let retry_venue_order_id = VenueOrderId::from("V-REJECT-RETRY-2");
    let existing_trade_id = TradeId::from("T-REJECT-EXISTING");
    let trade_id = TradeId::from("T-REJECT-RETRY");

    let instrument = test_instrument();
    ctx.add_instrument(instrument.clone());
    let mut order = create_accepted_order(
        client_order_id.as_str(),
        instrument_id,
        OrderSide::Buy,
        "1.0",
        "3000.00",
        venue_order_id,
    );
    let existing_fill = TestOrderEventStubs::filled(
        &order,
        &instrument,
        Some(existing_trade_id),
        None,
        Some(Price::from("3000.00")),
        Some(Quantity::from("1.0")),
        Some(LiquiditySide::Maker),
        None,
        None,
        Some(test_account_id()),
    );
    order.apply(existing_fill).unwrap();
    ctx.add_order(order);

    let rejected_report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("2.0"),
        Quantity::from("2.0"),
    );
    let rejected_fill = create_fill_report(
        client_order_id,
        venue_order_id,
        instrument_id,
        trade_id,
        "1.0",
    );
    let rejected = ctx.manager.reconcile_execution_mass_status(
        &create_mass_status(vec![rejected_report], vec![rejected_fill]),
        &ctx.exec_engine,
    );

    assert!(rejected.events.is_empty());
    let rejected_order = ctx.get_order(&client_order_id).unwrap();
    assert_eq!(rejected_order.trade_ids(), vec![&existing_trade_id]);

    ctx.add_order(create_accepted_order(
        retry_client_order_id.as_str(),
        instrument_id,
        OrderSide::Buy,
        "1.0",
        "3000.00",
        retry_venue_order_id,
    ));
    let retry_fill = create_fill_report(
        retry_client_order_id,
        retry_venue_order_id,
        instrument_id,
        trade_id,
        "1.0",
    );
    let retry = ctx.manager.reconcile_execution_mass_status(
        &create_mass_status(vec![], vec![retry_fill]),
        &ctx.exec_engine,
    );

    assert_eq!(retry.events.len(), 1);
    assert!(matches!(retry.events[0], OrderEventAny::Filled(_)));
    let retry_order = ctx.get_order(&retry_client_order_id).unwrap();
    assert_eq!(retry_order.trade_ids(), vec![&trade_id]);
}

#[rstest]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_reconciliation_fill_dispatch_rejection_does_not_commit() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-DISPATCH-RETRY");
    let venue_order_id = VenueOrderId::from("V-DISPATCH-RETRY");
    let trade_id = TradeId::from("T-DISPATCH-RETRY");

    ctx.add_instrument(test_instrument());
    ctx.add_order(create_limit_order(
        client_order_id.as_str(),
        instrument_id,
        OrderSide::Buy,
        "1.0",
        "3000.00",
    ));

    let fill = create_fill_report(
        client_order_id,
        venue_order_id,
        instrument_id,
        trade_id,
        "1.0",
    );
    // An orphan report queues without the order-report working.apply projection.
    let rejected = ctx.manager.reconcile_execution_mass_status(
        &create_mass_status(vec![], vec![fill.clone()]),
        &ctx.exec_engine,
    );

    assert_eq!(
        rejected
            .events
            .iter()
            .filter(|event| matches!(event, OrderEventAny::Filled(_)))
            .count(),
        1
    );
    assert!(
        !ctx.get_order(&client_order_id)
            .unwrap()
            .trade_ids()
            .contains(&&trade_id)
    );

    let order = ctx.get_order(&client_order_id).unwrap();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    let order = ctx.cache.borrow_mut().update_order(&submitted).unwrap();
    let accepted = TestOrderEventStubs::accepted(&order, test_account_id(), venue_order_id);
    ctx.cache.borrow_mut().update_order(&accepted).unwrap();

    let retry = ctx
        .manager
        .reconcile_execution_mass_status(&create_mass_status(vec![], vec![fill]), &ctx.exec_engine);

    assert_eq!(
        retry
            .events
            .iter()
            .filter(|event| matches!(event, OrderEventAny::Filled(_)))
            .count(),
        1
    );
    assert!(
        ctx.get_order(&client_order_id)
            .unwrap()
            .trade_ids()
            .contains(&&trade_id)
    );
}

#[rstest]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_applied_reconciliation_fill_is_committed() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let trade_id = TradeId::from("T-COMMITTED");
    let first_client_order_id = ClientOrderId::from("O-COMMITTED-1");
    let first_venue_order_id = VenueOrderId::from("V-COMMITTED-1");
    let second_client_order_id = ClientOrderId::from("O-COMMITTED-2");
    let second_venue_order_id = VenueOrderId::from("V-COMMITTED-2");

    ctx.add_instrument(test_instrument());
    ctx.add_order(create_accepted_order(
        first_client_order_id.as_str(),
        instrument_id,
        OrderSide::Buy,
        "1.0",
        "3000.00",
        first_venue_order_id,
    ));
    ctx.add_order(create_accepted_order(
        second_client_order_id.as_str(),
        instrument_id,
        OrderSide::Buy,
        "1.0",
        "3000.00",
        second_venue_order_id,
    ));

    let first = ctx.manager.reconcile_execution_mass_status(
        &create_mass_status(
            vec![],
            vec![create_fill_report(
                first_client_order_id,
                first_venue_order_id,
                instrument_id,
                trade_id,
                "1.0",
            )],
        ),
        &ctx.exec_engine,
    );
    assert_eq!(first.events.len(), 1);

    let duplicate = ctx.manager.reconcile_execution_mass_status(
        &create_mass_status(
            vec![],
            vec![create_fill_report(
                second_client_order_id,
                second_venue_order_id,
                instrument_id,
                trade_id,
                "1.0",
            )],
        ),
        &ctx.exec_engine,
    );

    assert!(duplicate.events.is_empty());
    assert!(
        ctx.get_order(&second_client_order_id)
            .unwrap()
            .trade_ids()
            .is_empty()
    );
}

#[rstest]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_same_cycle_cross_order_fill_is_queued_once() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let trade_id = TradeId::from("T-CROSS-ORDER");
    let first_client_order_id = ClientOrderId::from("O-CROSS-ORDER-1");
    let first_venue_order_id = VenueOrderId::from("V-CROSS-ORDER-1");
    let second_client_order_id = ClientOrderId::from("O-CROSS-ORDER-2");
    let second_venue_order_id = VenueOrderId::from("V-CROSS-ORDER-2");

    ctx.add_instrument(test_instrument());
    ctx.add_order(create_accepted_order(
        first_client_order_id.as_str(),
        instrument_id,
        OrderSide::Buy,
        "1.0",
        "3000.00",
        first_venue_order_id,
    ));
    ctx.add_order(create_accepted_order(
        second_client_order_id.as_str(),
        instrument_id,
        OrderSide::Buy,
        "1.0",
        "3000.00",
        second_venue_order_id,
    ));

    let result = ctx.manager.reconcile_execution_mass_status(
        &create_mass_status(
            vec![],
            vec![
                create_fill_report(
                    first_client_order_id,
                    first_venue_order_id,
                    instrument_id,
                    trade_id,
                    "1.0",
                ),
                create_fill_report(
                    second_client_order_id,
                    second_venue_order_id,
                    instrument_id,
                    trade_id,
                    "1.0",
                ),
            ],
        ),
        &ctx.exec_engine,
    );

    assert_eq!(
        result
            .events
            .iter()
            .filter(|event| matches!(event, OrderEventAny::Filled(_)))
            .count(),
        1
    );

    let applied_orders = [first_client_order_id, second_client_order_id]
        .iter()
        .filter(|client_order_id| {
            ctx.get_order(client_order_id)
                .unwrap()
                .trade_ids()
                .contains(&&trade_id)
        })
        .count();

    assert_eq!(applied_orders, 1);
}

#[rstest]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_inferred_fill_is_not_committed_as_reported_fill() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let external_venue_order_id = VenueOrderId::from("V-INFERRED-SOURCE");
    ctx.add_instrument(test_instrument());

    let order_report = create_order_report(
        None,
        external_venue_order_id,
        instrument_id,
        OrderStatus::Filled,
        Quantity::from("2.0"),
        Quantity::from("2.0"),
    )
    .with_avg_px(dec!(3000.0));
    let real_trade_id = TradeId::from("T-INFERRED-SOURCE");
    let source = ctx.manager.reconcile_execution_mass_status(
        &create_mass_status(
            vec![order_report],
            vec![create_fill_report(
                ClientOrderId::from(external_venue_order_id.as_str()),
                external_venue_order_id,
                instrument_id,
                real_trade_id,
                "1.0",
            )],
        ),
        &ctx.exec_engine,
    );

    let inferred_trade_id = source
        .events
        .iter()
        .find_map(|event| match event {
            OrderEventAny::Filled(fill) if fill.trade_id != real_trade_id => Some(fill.trade_id),
            _ => None,
        })
        .expect("expected inferred fill");

    let client_order_id = ClientOrderId::from("O-INFERRED-REUSE");
    let venue_order_id = VenueOrderId::from("V-INFERRED-REUSE");
    ctx.add_order(create_accepted_order(
        client_order_id.as_str(),
        instrument_id,
        OrderSide::Buy,
        "1.0",
        "3000.00",
        venue_order_id,
    ));
    let reused = ctx.manager.reconcile_execution_mass_status(
        &create_mass_status(
            vec![],
            vec![create_fill_report(
                client_order_id,
                venue_order_id,
                instrument_id,
                inferred_trade_id,
                "1.0",
            )],
        ),
        &ctx.exec_engine,
    );

    assert_eq!(reused.events.len(), 1);
    assert!(matches!(reused.events[0], OrderEventAny::Filled(_)));
}

#[rstest]
#[case(Some(0), 60, true)]
#[case(Some(2), 120, true)]
#[case(None, 86_400, false)]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_processed_fill_retention(
    #[case] lookback_mins: Option<u64>,
    #[case] horizon_secs: u64,
    #[case] prunes_past_horizon: bool,
) {
    let config = ExecutionManagerConfig {
        lookback_mins,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    let trade_id = TradeId::from("T-RETENTION");
    let first_client_order_id = ClientOrderId::from("O-RETENTION-1");
    let first_venue_order_id = VenueOrderId::from("V-RETENTION-1");
    let second_client_order_id = ClientOrderId::from("O-RETENTION-2");
    let second_venue_order_id = VenueOrderId::from("V-RETENTION-2");

    ctx.add_instrument(test_instrument());
    ctx.add_order(create_accepted_order(
        first_client_order_id.as_str(),
        instrument_id,
        OrderSide::Buy,
        "1.0",
        "3000.00",
        first_venue_order_id,
    ));
    ctx.add_order(create_accepted_order(
        second_client_order_id.as_str(),
        instrument_id,
        OrderSide::Buy,
        "1.0",
        "3000.00",
        second_venue_order_id,
    ));

    let first = ctx.manager.reconcile_execution_mass_status(
        &create_mass_status(
            vec![],
            vec![create_fill_report(
                first_client_order_id,
                first_venue_order_id,
                instrument_id,
                trade_id,
                "1.0",
            )],
        ),
        &ctx.exec_engine,
    );
    assert_eq!(first.events.len(), 1);

    ctx.advance_both(dst::time::Duration::from_secs(horizon_secs))
        .await;
    ctx.manager.prune_processed_fills();
    let at_horizon = ctx.manager.reconcile_execution_mass_status(
        &create_mass_status(
            vec![],
            vec![create_fill_report(
                second_client_order_id,
                second_venue_order_id,
                instrument_id,
                trade_id,
                "1.0",
            )],
        ),
        &ctx.exec_engine,
    );
    assert!(at_horizon.events.is_empty());

    ctx.advance_both(dst::time::Duration::from_nanos(1)).await;
    ctx.manager.prune_processed_fills();
    let past_horizon = ctx.manager.reconcile_execution_mass_status(
        &create_mass_status(
            vec![],
            vec![create_fill_report(
                second_client_order_id,
                second_venue_order_id,
                instrument_id,
                trade_id,
                "1.0",
            )],
        ),
        &ctx.exec_engine,
    );
    assert_eq!(past_horizon.events.len(), usize::from(prunes_past_horizon));
}

#[tokio::test]
async fn test_orphan_fills_without_order_reports_processed() {
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

    let orphan_venue_order_id = VenueOrderId::from("V-ORPHAN-001");
    let venue_position_id = PositionId::from("ETHUSDT-PERP.BINANCE-LONG");

    let orphan_fill = FillReport::new(
        test_account_id(),
        instrument_id,
        orphan_venue_order_id,
        TradeId::from("T-ORPHAN-001"),
        OrderSide::Buy,
        Quantity::from("0.75"),
        Price::from("3000.00"),
        Money::from("0.20 USDT"),
        LiquiditySide::Taker,
        None,
        Some(venue_position_id),
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
        None,
    );

    let second_fill = FillReport::new(
        test_account_id(),
        instrument_id,
        orphan_venue_order_id,
        TradeId::from("T-ORPHAN-002"),
        OrderSide::Buy,
        Quantity::from("1.25"),
        Price::from("3100.00"),
        Money::from("0.30 USDT"),
        LiquiditySide::Maker,
        None,
        Some(venue_position_id),
        UnixNanos::from(2_000),
        UnixNanos::from(2_000),
        None,
    );

    mass_status.add_fill_reports(vec![orphan_fill, second_fill]);

    let replay_status = mass_status.clone();

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(result.events.len(), 3);
    assert!(matches!(result.events[0], OrderEventAny::Accepted(_)));
    assert!(matches!(result.events[1], OrderEventAny::Filled(_)));
    assert!(matches!(result.events[2], OrderEventAny::Filled(_)));

    let position = ctx
        .cache
        .borrow()
        .position_owned(&venue_position_id)
        .unwrap();

    assert_eq!(position.id, venue_position_id);
    assert_eq!(position.instrument_id, instrument_id);
    assert_eq!(position.side, PositionSide::Long);
    assert_eq!(position.quantity, Quantity::from("2.0"));
    assert_eq!(position.avg_px_open, dec!(3062.5));

    let replay = ctx
        .manager
        .reconcile_execution_mass_status(&replay_status, &ctx.exec_engine);

    assert!(replay.events.is_empty());
}

#[tokio::test]
async fn test_orphan_fill_group_validates_when_later_fill_has_position_id() {
    let _log_guard = MANAGER_LOG_TEST_LOCK.lock().await;
    install_manager_log_capture();

    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let venue_order_id = VenueOrderId::from("V-MIXED-POSITION-001");

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    mass_status.add_fill_reports(vec![
        FillReport::new(
            test_account_id(),
            instrument_id,
            venue_order_id,
            TradeId::from("T-MIXED-POSITION-001"),
            OrderSide::Buy,
            Quantity::from("0.75"),
            Price::from("3000.00"),
            Money::from("0.20 USDT"),
            LiquiditySide::Taker,
            None,
            None,
            UnixNanos::from(1_000),
            UnixNanos::from(1_000),
            None,
        ),
        FillReport::new(
            test_account_id(),
            instrument_id,
            venue_order_id,
            TradeId::from("T-MIXED-POSITION-002"),
            OrderSide::Buy,
            Quantity::from("1.25"),
            Price::from("3100.00"),
            Money::from("0.30 USDT"),
            LiquiditySide::Maker,
            None,
            Some(PositionId::from("ETHUSDT-PERP.BINANCE-LONG")),
            UnixNanos::from(2_000),
            UnixNanos::from(2_000),
            None,
        ),
    ]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let messages = MANAGER_LOG_CAPTURE.messages.lock().clone();
    let cache = ctx.cache.borrow();

    assert!(result.events.is_empty());
    assert!(result.external_orders.is_empty());
    assert_eq!(cache.orders_total_count(None, None, None, None, None), 0);
    assert!(cache.positions(None, None, None, None, None).is_empty());

    assert_eq!(
        messages,
        vec![
            "Cannot materialize orphan fills for venue order V-MIXED-POSITION-001: venue position ID is missing"
                .to_string(),
        ]
    );
}

#[tokio::test]
async fn test_orphan_fills_for_unknown_instrument_skipped() {
    let _log_guard = MANAGER_LOG_TEST_LOCK.lock().await;
    install_manager_log_capture();

    let mut ctx = TestContext::new();
    ctx.add_instrument(test_instrument());

    let unknown_instrument_id = InstrumentId::from("UNKNOWN-PERP.BINANCE");

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let orphan_fill = FillReport::new(
        test_account_id(),
        unknown_instrument_id,
        VenueOrderId::from("V-UNKNOWN-001"),
        TradeId::from("T-UNKNOWN-001"),
        OrderSide::Buy,
        Quantity::from("1.0"),
        Price::from("100.00"),
        Money::from("0.10 USDT"),
        LiquiditySide::Taker,
        None,
        Some(PositionId::from("UNKNOWN-PERP.BINANCE-LONG")),
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
        None,
    );

    mass_status.add_fill_reports(vec![orphan_fill]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let messages = MANAGER_LOG_CAPTURE.messages.lock().clone();
    let cache = ctx.cache.borrow();

    assert!(result.events.is_empty());
    assert!(result.external_orders.is_empty());
    assert_eq!(cache.orders_total_count(None, None, None, None, None), 0);
    assert!(cache.positions(None, None, None, None, None).is_empty());

    assert!(
        messages
            .iter()
            .any(|message| message == "1 orders skipped (instrument not in cache)")
    );
}

#[tokio::test]
async fn test_orphan_fills_without_position_id_dropped() {
    let _log_guard = MANAGER_LOG_TEST_LOCK.lock().await;
    install_manager_log_capture();

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

    let orphan_fill = |venue_order_id: &str, trade_id: &str, ts: u64| {
        FillReport::new(
            test_account_id(),
            instrument_id,
            VenueOrderId::from(venue_order_id),
            TradeId::from(trade_id),
            OrderSide::Buy,
            Quantity::from("0.50"),
            Price::from("3000.00"),
            Money::from("0.10 USDT"),
            LiquiditySide::Taker,
            None,
            None,
            UnixNanos::from(ts),
            UnixNanos::from(ts),
            None,
        )
    };

    mass_status.add_fill_reports(vec![
        orphan_fill("V-NETTING-001", "T-NETTING-001", 1_000),
        orphan_fill("V-NETTING-001", "T-NETTING-002", 2_000),
        orphan_fill("V-NETTING-002", "T-NETTING-003", 3_000),
        orphan_fill("V-NETTING-003", "T-NETTING-004", 4_000),
        orphan_fill("V-NETTING-004", "T-NETTING-005", 5_000),
        orphan_fill("V-NETTING-005", "T-NETTING-006", 6_000),
        orphan_fill("V-NETTING-006", "T-NETTING-007", 7_000),
    ]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    let messages = MANAGER_LOG_CAPTURE.messages.lock().clone();
    let cache = ctx.cache.borrow();

    assert!(result.events.is_empty());
    assert!(result.external_orders.is_empty());
    assert!(result.unresolved_positions.is_empty());
    assert_eq!(cache.orders_total_count(None, None, None, None, None), 0);
    assert!(cache.positions(None, None, None, None, None).is_empty());
    assert_eq!(
        messages,
        vec![
            "Dropped 7 fill(s) in 6 fill group(s) without an order report or cached order \
             (V-NETTING-001 (ETHUSDT-PERP.BINANCE), V-NETTING-002 (ETHUSDT-PERP.BINANCE), \
             V-NETTING-003 (ETHUSDT-PERP.BINANCE), V-NETTING-004 (ETHUSDT-PERP.BINANCE), \
             V-NETTING-005 (ETHUSDT-PERP.BINANCE))"
                .to_string(),
        ]
    );
}

#[tokio::test]
async fn test_filtered_client_order_ids_skips_orphan_fills() {
    // Orphan fills (fills without order reports) should also be filtered
    let filtered_id = ClientOrderId::from("O-FILTERED-002");
    let venue_order_id = VenueOrderId::from("V-FILTERED-002");

    let config = ExecutionManagerConfig {
        filtered_client_order_ids: IndexSet::from([filtered_id]),
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    // Add the order to cache (simulating an order placed before filtering was enabled)
    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(filtered_id)
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

    // Add orphan fill (fill without order report) for the filtered order
    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-001"),
        OrderSide::Buy,
        Quantity::from("5.0"),
        Price::from("100.00"),
        Money::from("0.50 USD"),
        LiquiditySide::Taker,
        Some(filtered_id),
        None,
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
        None,
    );

    mass_status.add_fill_reports(vec![fill]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // No fill events should be generated for filtered order
    assert!(
        !result
            .events
            .iter()
            .any(|e| matches!(e, OrderEventAny::Filled(_))),
        "Filtered order should not receive orphan fill events"
    );
}

#[tokio::test]
async fn test_filtered_client_order_ids_skips_orphan_fills_via_venue_order_id_lookup() {
    // Orphan fills looked up by venue_order_id should also be filtered
    let filtered_id = ClientOrderId::from("O-FILTERED-003");
    let venue_order_id = VenueOrderId::from("V-FILTERED-003");

    let config = ExecutionManagerConfig {
        filtered_client_order_ids: IndexSet::from([filtered_id]),
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    ctx.add_instrument(test_instrument());

    let mut order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(filtered_id)
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

    // Add orphan fill WITHOUT client_order_id (will be looked up by venue_order_id)
    let fill = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        TradeId::from("T-002"),
        OrderSide::Buy,
        Quantity::from("5.0"),
        Price::from("100.00"),
        Money::from("0.50 USD"),
        LiquiditySide::Taker,
        None, // No client_order_id - will use venue_order_id lookup
        None,
        UnixNanos::from(1_000),
        UnixNanos::from(1_000),
        None,
    );

    mass_status.add_fill_reports(vec![fill]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    // No fill events should be generated for filtered order
    assert!(
        !result
            .events
            .iter()
            .any(|e| matches!(e, OrderEventAny::Filled(_))),
        "Filtered order should not receive orphan fill events via venue_order_id lookup"
    );
}

#[rstest]
#[case::unbounded(false)]
#[case::bounded(true)]
#[tokio::test]
async fn test_mass_status_zero_quantity_fill_does_not_consume_trade_id(#[case] bounded: bool) {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let trade_id = TradeId::from("T-ZERO-RETRY");
    ctx.add_instrument(test_instrument());
    let order = create_limit_order("O-001", instrument_id, OrderSide::Buy, "2.0", "3000.00");
    ctx.add_order(order.clone());

    let valid = FillReport::new(
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

    let mut zero = valid.clone();
    zero.last_qty = Quantity::zero(1);
    zero.commission = Money::from("123.45 USDT");

    let mut mass_status = ExecutionMassStatus::new(
        test_client_id(),
        test_account_id(),
        test_venue(),
        UnixNanos::default(),
        None,
    );

    if bounded {
        mass_status.set_report_window(Some(UnixNanos::from(1)), true);
    }

    mass_status.add_fill_reports(vec![zero]);

    let rejected = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert!(rejected.events.is_empty());
    assert_eq!(ctx.get_order(&client_order_id), Some(order));
    assert_eq!(
        ctx.cache
            .borrow()
            .positions_total_count(None, None, None, None, None),
        0
    );

    mass_status.add_fill_reports(vec![valid]);
    let accepted = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert_eq!(accepted.events.len(), 1);

    let OrderEventAny::Filled(fill) = &accepted.events[0] else {
        panic!("Expected fill");
    };

    assert_eq!(fill.last_qty, Quantity::from("1.0"));
    assert_eq!(fill.commission, Some(Money::from("0.50 USDT")));
    assert_eq!(fill.trade_id, trade_id);
}

#[rstest]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_reconciled_fill_delivery_schedules() {
    #[derive(Clone, Copy, Debug)]
    enum Delivery {
        ZeroFirst,
        First,
        Second,
    }

    use Delivery::{First, Second, ZeroFirst};

    let client_order_id = ClientOrderId::from("O-FILL-SCHEDULE");
    let venue_order_id = VenueOrderId::from("V-FILL-SCHEDULE");
    let instrument_id = test_instrument_id();
    let position_id = PositionId::from("P-FILL-SCHEDULE");
    let mut first = create_fill_report(
        client_order_id,
        venue_order_id,
        instrument_id,
        TradeId::from("T-1"),
        "1.0",
    );
    first.venue_position_id = Some(position_id);
    let mut second = create_fill_report(
        client_order_id,
        venue_order_id,
        instrument_id,
        TradeId::from("T-2"),
        "2.0",
    );
    second.venue_position_id = Some(position_id);
    second.last_px = Price::from("2997.00");
    second.commission = Money::from("0.80 USDT");
    let mut zero = first.clone();
    zero.last_qty = Quantity::zero(1);
    zero.commission = Money::from("123.45 USDT");

    for schedule in recovery_schedules([ZeroFirst, First, Second], 8) {
        let mut ctx = TestContext::new();
        ctx.add_instrument(test_instrument());
        ctx.add_order(create_accepted_order(
            client_order_id.as_str(),
            instrument_id,
            OrderSide::Buy,
            "3.0",
            "3000.00",
            venue_order_id,
        ));
        let mut ledger = FillLedger::default();

        for delivery in &schedule {
            let fill = match delivery {
                ZeroFirst => &zero,
                First => &first,
                Second => &second,
            };

            let apply = !fill.last_qty.is_zero() && !ledger.trades.contains(&fill.trade_id);
            let result = ctx.manager.reconcile_execution_mass_status(
                &create_mass_status(vec![], vec![fill.clone()]),
                &ctx.exec_engine,
            );
            assert_eq!(
                result.events.len(),
                usize::from(apply),
                "{schedule:?}: {:?}",
                result.events
            );

            if apply {
                let [OrderEventAny::Filled(event)] = result.events.as_slice() else {
                    panic!("Expected a fill in {schedule:?}: {:?}", result.events);
                };

                assert_eq!(event.trade_id, fill.trade_id, "{schedule:?}");
                assert_eq!(event.last_qty, fill.last_qty, "{schedule:?}");
                assert_eq!(event.last_px, fill.last_px, "{schedule:?}");
                assert_eq!(event.commission, Some(fill.commission), "{schedule:?}");
                ledger.trades.push(fill.trade_id);
                ledger.qty += fill.last_qty.as_decimal();
                ledger.value += fill.last_qty.as_decimal() * fill.last_px.as_decimal();
                ledger.commission += fill.commission.as_decimal();
            }

            ledger.assert_matches(&ctx, client_order_id, position_id, &schedule);
        }
    }
}

#[derive(Default)]
struct FillLedger {
    trades: Vec<TradeId>,
    qty: Decimal,
    value: Decimal,
    commission: Decimal,
}

impl FillLedger {
    fn assert_matches(
        &self,
        ctx: &TestContext,
        client_order_id: ClientOrderId,
        position_id: PositionId,
        schedule: &[impl Debug],
    ) {
        let order = ctx
            .get_order(&client_order_id)
            .unwrap_or_else(|| panic!("Missing order {client_order_id} in {schedule:?}"));

        let status = match self.trades.len() {
            0 => OrderStatus::Accepted,
            1 => OrderStatus::PartiallyFilled,
            _ => OrderStatus::Filled,
        };

        let avg_px = (!self.qty.is_zero()).then(|| self.value / self.qty);
        let fees = order
            .commissions()
            .get(&Currency::USDT())
            .map(Money::as_decimal);
        assert_eq!(order.status(), status, "{schedule:?}");
        assert_eq!(
            order.trade_ids(),
            self.trades.iter().collect::<Vec<_>>(),
            "{schedule:?}"
        );
        assert_eq!(order.filled_qty().as_decimal(), self.qty, "{schedule:?}");
        assert_eq!(order.avg_px(), avg_px, "{schedule:?}");
        assert_eq!(
            fees,
            (!self.trades.is_empty()).then_some(self.commission),
            "{schedule:?}"
        );
        let cache = ctx.cache.borrow();
        let positions = cache.positions(None, None, None, None, None);
        assert_eq!(
            positions.len(),
            usize::from(!self.qty.is_zero()),
            "{schedule:?}"
        );

        if let [position] = positions.as_slice() {
            assert_eq!(position.id, position_id, "{schedule:?}");
            assert_eq!(position.signed_qty, self.qty, "{schedule:?}");
            assert_eq!(Some(position.avg_px_open), avg_px, "{schedule:?}");
            assert_eq!(
                position
                    .commissions
                    .get(&Currency::USDT())
                    .map(Money::as_decimal),
                Some(self.commission),
                "{schedule:?}"
            );
        }
    }
}

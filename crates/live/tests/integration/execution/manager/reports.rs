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
fn test_fill_deduplication_new_fill_not_processed() {
    let ctx = TestContext::new();
    let trade_id = TradeId::from("T-001");

    assert!(!ctx.manager.is_fill_recently_processed(
        test_account_id(),
        test_instrument_id(),
        trade_id
    ));
}

#[rstest]
fn test_fill_deduplication_tracks_processed_fill() {
    let mut ctx = TestContext::new();
    let trade_id = TradeId::from("T-001");

    ctx.manager
        .mark_fill_processed(test_account_id(), test_instrument_id(), trade_id);

    assert!(ctx.manager.is_fill_recently_processed(
        test_account_id(),
        test_instrument_id(),
        trade_id
    ));
    assert!(!ctx.manager.is_fill_recently_processed(
        test_account_id(),
        test_instrument_id2(),
        trade_id
    ));
    assert!(!ctx.manager.is_fill_recently_processed(
        AccountId::from("BINANCE-002"),
        test_instrument_id(),
        trade_id
    ));
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_fill_deduplication_prune_removes_expired() {
    let mut ctx = TestContext::new();
    let old_trade = TradeId::from("T-OLD");
    let new_trade = TradeId::from("T-NEW");

    ctx.manager
        .mark_fill_processed(test_account_id(), test_instrument_id(), old_trade);
    ctx.advance_both(dst::time::Duration::from_secs(120)).await;
    ctx.manager
        .mark_fill_processed(test_account_id(), test_instrument_id(), new_trade);

    ctx.manager.prune_recent_fills_cache(60.0); // 60 second TTL

    assert!(!ctx.manager.is_fill_recently_processed(
        test_account_id(),
        test_instrument_id(),
        old_trade
    ));
    assert!(ctx.manager.is_fill_recently_processed(
        test_account_id(),
        test_instrument_id(),
        new_trade
    ));
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_fill_deduplication_prune_uses_monotonic_ttl() {
    let mut ctx = TestContext::new();
    let trade_id = TradeId::from("T-MONOTONIC");

    ctx.manager
        .mark_fill_processed(test_account_id(), test_instrument_id(), trade_id);
    ctx.advance_time(120_000_000_000);
    ctx.manager.prune_recent_fills_cache(60.0);
    assert!(ctx.manager.is_fill_recently_processed(
        test_account_id(),
        test_instrument_id(),
        trade_id
    ));

    advance_clock(dst::time::Duration::from_secs(61)).await;
    ctx.manager.prune_recent_fills_cache(60.0);
    assert!(!ctx.manager.is_fill_recently_processed(
        test_account_id(),
        test_instrument_id(),
        trade_id
    ));
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_prune_recent_fills_cache_keeps_all_on_overflow_ttl() {
    // Positive-overflow and infinity TTLs must saturate to keep-all.
    let mut ctx = TestContext::new();
    let trade_id = TradeId::from("T-KEEP");
    ctx.manager
        .mark_fill_processed(test_account_id(), test_instrument_id(), trade_id);
    advance_clock(dst::time::Duration::from_nanos(1)).await;

    ctx.manager.prune_recent_fills_cache(1.0e30);
    assert!(
        ctx.manager
            .is_fill_recently_processed(test_account_id(), test_instrument_id(), trade_id),
        "overflowing TTL must keep entries, not prune them",
    );

    ctx.manager.prune_recent_fills_cache(f64::INFINITY);
    assert!(
        ctx.manager
            .is_fill_recently_processed(test_account_id(), test_instrument_id(), trade_id),
        "infinite TTL must keep entries",
    );
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_prune_recent_fills_cache_prunes_all_on_negative_or_nan_ttl() {
    // Negative and NaN TTLs fall back to a zero TTL, matching the old cast.
    let mut ctx = TestContext::new();

    ctx.manager.mark_fill_processed(
        test_account_id(),
        test_instrument_id(),
        TradeId::from("T-NEG"),
    );
    advance_clock(dst::time::Duration::from_nanos(1)).await;
    ctx.manager.prune_recent_fills_cache(-1.0);
    assert!(!ctx.manager.is_fill_recently_processed(
        test_account_id(),
        test_instrument_id(),
        TradeId::from("T-NEG"),
    ));

    ctx.manager.mark_fill_processed(
        test_account_id(),
        test_instrument_id(),
        TradeId::from("T-NAN"),
    );
    advance_clock(dst::time::Duration::from_nanos(1)).await;
    ctx.manager.prune_recent_fills_cache(f64::NAN);
    assert!(!ctx.manager.is_fill_recently_processed(
        test_account_id(),
        test_instrument_id(),
        TradeId::from("T-NAN"),
    ));
}

#[rstest]
fn test_observe_fill_report_does_not_mark_fill_processed() {
    // Incoming fill reports record activity only. Normal-live fill dedup marking
    // follows the separate order-event observation path.
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let trade_id = TradeId::from("T-001");

    ctx.add_instrument(test_instrument());

    let fill_report = FillReport::new(
        test_account_id(),
        instrument_id,
        VenueOrderId::from("V-001"),
        trade_id,
        OrderSide::Buy,
        Quantity::from("1.0"),
        Price::from("3000.00"),
        Money::new(0.0, Currency::USDT()),
        LiquiditySide::Maker,
        Some(ClientOrderId::from("O-001")),
        None, // venue_position_id
        UnixNanos::from(2_000_000),
        UnixNanos::from(2_000_000),
        None, // report_id
    );

    let report = ExecutionReport::Fill(Box::new(fill_report));

    ctx.manager.observe_execution_report(&report);

    // observe_execution_report should not mark fills as processed.
    assert!(
        !ctx.manager
            .is_fill_recently_processed(test_account_id(), instrument_id, trade_id)
    );
}

#[rstest]
fn test_observe_fill_report_without_client_order_id_uses_cache_fallback() {
    let config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 5,
        open_check_threshold_ns: DurationNanos::from_secs(1),
        max_single_order_queries_per_cycle: 5,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");
    let venue_order_id = VenueOrderId::from("V-001");
    let trade_id = TradeId::from("T-FALLBACK");

    ctx.add_instrument(test_instrument());
    insert_accepted_limit_order(&ctx, client_order_id, venue_order_id, test_client_id());

    // Register inflight so we can verify activity recording deferred the check
    ctx.manager.register_inflight(client_order_id, UUID4::new());

    // Create fill report without client_order_id
    let fill_report = FillReport::new(
        test_account_id(),
        instrument_id,
        venue_order_id,
        trade_id,
        OrderSide::Buy,
        Quantity::from("1.0"),
        Price::from("3000.00"),
        Money::new(0.0, Currency::USDT()),
        LiquiditySide::Maker,
        None, // No client_order_id: triggers cache fallback
        None,
        UnixNanos::from(2_000_000),
        UnixNanos::from(2_000_000),
        None,
    );

    let report = ExecutionReport::Fill(Box::new(fill_report));

    // observe should resolve client_order_id via cache and record activity
    ctx.manager.observe_execution_report(&report);

    let queries = ctx.manager.check_open_order_queries();
    assert!(queries.is_empty());
}

#[tokio::test]
async fn test_zero_quantity_fill_does_not_refresh_recency() {
    let mut ctx = TestContext::new();
    let instrument = test_instrument();
    let trade_id = TradeId::from("T-ZERO-RECENCY");
    let mut order = create_accepted_order(
        "O-ZERO-RECENCY",
        instrument.id(),
        OrderSide::Buy,
        "2.0",
        "3000.00",
        VenueOrderId::from("V-ZERO-RECENCY"),
    );
    let event = TestOrderEventStubs::filled(
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
    order.apply(event.clone()).unwrap();
    ctx.add_order(order);

    let OrderEventAny::Filled(fill) = event else {
        panic!("Expected fill");
    };

    let mut zero = fill.clone();
    zero.last_qty = Quantity::zero(1);

    ctx.manager.commit_recent_fill_if_applied(&zero);

    assert!(
        !ctx.manager
            .is_fill_recently_processed(test_account_id(), instrument.id(), trade_id)
    );

    ctx.manager.commit_recent_fill_if_applied(&fill);

    assert!(
        ctx.manager
            .is_fill_recently_processed(test_account_id(), instrument.id(), trade_id)
    );
}

#[rstest]
#[case::unchanged(None, "3.000", "0.75 USDT")]
#[case::partial_opening(Some("0.500"), "2.500", "0.625 USDT")]
#[case::entire_opening(Some("1.000"), "2.000", "0.50 USDT")]
#[case::partial_closing(Some("1.500"), "1.500", "0.375 USDT")]
fn test_position_fill_report_accounts_for_virtual_flip(
    #[case] voided_qty: Option<&str>,
    #[case] remaining_qty: &str,
    #[case] remaining_commission: &str,
    #[values(false, true)] rebuild_index: bool,
) {
    let ctx = TestContext::new();
    let instrument = test_instrument();
    let position_id = PositionId::from("P-FLIP-SOURCE");
    let position =
        create_test_position(&instrument, position_id, OrderSide::Buy, "2.000", "3000.00");
    ctx.add_instrument(instrument.clone());
    ctx.add_position(&position);

    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument.id())
        .client_order_id(ClientOrderId::from("O-FLIP-ROUTE"))
        .side(OrderSide::Sell)
        .quantity(Quantity::from("5.000"))
        .build();
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(order.strategy_id(), OmsType::Hedging);
    ctx.add_order(order.clone());
    ctx.exec_engine
        .borrow_mut()
        .process(&TestOrderEventStubs::submitted(&order, test_account_id()));
    ctx.exec_engine
        .borrow_mut()
        .process(&TestOrderEventStubs::accepted(
            &order,
            test_account_id(),
            VenueOrderId::from("V-FLIP-ROUTE"),
        ));
    let fill = TestOrderEventStubs::filled(
        &order,
        &instrument,
        Some(TradeId::from("T-FLIP-ROUTE")),
        Some(position_id),
        Some(Price::from("3100.00")),
        Some(Quantity::from("3.000")),
        Some(LiquiditySide::Taker),
        Some(Money::from("0.75 USDT")),
        None,
        Some(test_account_id()),
    );
    ctx.exec_engine.borrow_mut().process(&fill);

    let OrderEventAny::Filled(fill) = fill else {
        unreachable!()
    };

    let mut report = FillReport::new(
        fill.account_id,
        fill.instrument_id,
        fill.venue_order_id,
        fill.trade_id,
        fill.order_side,
        fill.last_qty,
        fill.last_px,
        fill.commission.unwrap(),
        fill.liquidity_side,
        Some(fill.client_order_id),
        Some(position_id),
        fill.ts_event,
        fill.ts_init,
        None,
    );

    if let Some(voided_qty) = voided_qty {
        let voided_qty = Quantity::from(voided_qty);
        let voided = OrderFillVoidedSpec::builder()
            .trader_id(fill.trader_id)
            .strategy_id(fill.strategy_id)
            .instrument_id(fill.instrument_id)
            .client_order_id(fill.client_order_id)
            .venue_order_id(fill.venue_order_id)
            .account_id(fill.account_id)
            .trade_id(fill.trade_id)
            .voided_qty(voided_qty)
            .order_side(fill.order_side)
            .order_type(fill.order_type)
            .last_px(fill.last_px)
            .currency(fill.currency)
            .liquidity_side(fill.liquidity_side)
            .position_id(position_id)
            .commission_voided(
                Money::from_decimal(voided_qty.as_decimal() * dec!(0.25), Currency::USDT())
                    .unwrap(),
            )
            .is_reopened(true)
            .build();
        ctx.exec_engine
            .borrow_mut()
            .process(&OrderEventAny::FillVoided(voided));
        assert!(!ctx.manager.position_contains_fill_report(&report));
        report.last_qty = Quantity::from(remaining_qty);
        report.commission = Money::from(remaining_commission);
    }

    if rebuild_index {
        let mut cache = ctx.cache.borrow_mut();
        cache.clear_index();
        cache.build_index();
    }

    assert!(ctx.manager.position_contains_fill_report(&report));
    ctx.manager
        .prepare_position_fill_report(&mut report, &[])
        .unwrap();
    assert_eq!(report.venue_position_id, Some(position_id));

    let accepted_report = report.clone();
    report.last_qty = Quantity::from("1.000");
    assert!(!ctx.manager.position_contains_fill_report(&report));
    report = accepted_report.clone();
    report.commission = Money::from("0.01 USDT");
    assert!(!ctx.manager.position_contains_fill_report(&report));
    report = accepted_report.clone();
    report.venue_position_id = Some(PositionId::from("P-UNRELATED"));
    assert!(!ctx.manager.position_contains_fill_report(&report));
    assert!(
        ctx.manager
            .prepare_position_fill_report(&mut report, &[])
            .is_err()
    );
    report = accepted_report;
    report.venue_position_id = None;
    ctx.manager
        .prepare_position_fill_report(&mut report, &[])
        .unwrap();
    assert_eq!(
        report.venue_position_id,
        ctx.cache
            .borrow()
            .position_id(&fill.client_order_id)
            .copied()
    );
    assert!(ctx.manager.position_contains_fill_report(&report));
}

#[rstest]
fn test_position_fill_report_preserves_netting_void_behavior(
    #[values(false, true)] rebuild_index: bool,
) {
    let ctx = TestContext::new();
    let instrument = test_instrument();

    let position_id = PositionId::new(format!(
        "{}-{}",
        instrument.id(),
        StrategyId::test_default()
    ));

    let position =
        create_test_position(&instrument, position_id, OrderSide::Buy, "2.000", "3000.00");
    let mut fill = position.events[0].clone();
    fill.commission = Some(Money::from("0.50 USDT"));
    let mut position = Position::new(&instrument, fill.clone());

    let voided = OrderFillVoidedSpec::builder()
        .trader_id(fill.trader_id)
        .strategy_id(fill.strategy_id)
        .instrument_id(fill.instrument_id)
        .client_order_id(fill.client_order_id)
        .venue_order_id(fill.venue_order_id)
        .account_id(fill.account_id)
        .trade_id(fill.trade_id)
        .voided_qty(Quantity::from("1.000"))
        .order_side(fill.order_side)
        .order_type(fill.order_type)
        .last_px(fill.last_px)
        .currency(fill.currency)
        .liquidity_side(fill.liquidity_side)
        .position_id(position_id)
        .commission_voided(Money::from("0.25 USDT"))
        .is_reopened(true)
        .build();
    position
        .apply_fill_void(
            voided,
            Quantity::from("1.000"),
            Some(Money::from("0.25 USDT")),
        )
        .unwrap();
    ctx.add_instrument(instrument);
    ctx.cache
        .borrow_mut()
        .add_position(&position, OmsType::Netting)
        .unwrap();

    if rebuild_index {
        let mut cache = ctx.cache.borrow_mut();
        cache.clear_index();
        cache.build_index();
    }

    let report = FillReport::new(
        fill.account_id,
        fill.instrument_id,
        fill.venue_order_id,
        fill.trade_id,
        fill.order_side,
        fill.last_qty,
        fill.last_px,
        fill.commission.unwrap(),
        fill.liquidity_side,
        Some(fill.client_order_id),
        None,
        fill.ts_event,
        fill.ts_init,
        None,
    );

    let mut corrected_report = report.clone();
    corrected_report.last_qty = Quantity::from("1.000");
    corrected_report.commission = Money::from("0.25 USDT");

    assert_eq!(position.quantity, Quantity::from("1.000"));
    assert!(ctx.manager.position_contains_fill_report(&report));
    assert!(!ctx.manager.position_contains_fill_report(&corrected_report));
}

#[rstest]
fn test_observe_position_report_records_activity() {
    let mut ctx = TestContext::new();
    let instrument_id = test_instrument_id();

    let position_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("1.0"),
        UnixNanos::from(3_000_000),
        UnixNanos::from(3_000_000),
        None, // report_id
        None, // venue_position_id
        Some(dec!(3000.0)),
    );

    let report = ExecutionReport::Position(Box::new(position_report));

    let key = (instrument_id, test_account_id());
    let other_account = (instrument_id, AccountId::from("BINANCE-002"));
    let other_instrument = (test_instrument_id2(), test_account_id());
    for (instrument_id, account_id) in [key, other_account, other_instrument] {
        ctx.manager
            .record_position_activity(instrument_id, account_id);
    }

    let check = ctx.manager.prepare_position_report_check(UUID4::new(), &[]);
    let before = [key, other_account, other_instrument]
        .map(|key| ctx.manager.position_report_check_is_current(&check, &key));

    ctx.manager.observe_execution_report(&report);

    assert_eq!(before, [true, true, true]);
    assert_eq!(
        [key, other_account, other_instrument]
            .map(|key| ctx.manager.position_report_check_is_current(&check, &key)),
        [false, true, true],
    );
}

#[rstest]
fn test_reconcile_order_status_report_publishes_external_order_initialized() {
    let ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let strategy_id = StrategyId::from("EXT-PUBLISH-DIRECT");

    ctx.add_instrument(test_instrument());
    ctx.exec_engine
        .borrow_mut()
        .register_external_order_claims(strategy_id, &HashSet::from([instrument_id]))
        .unwrap();

    let topic = switchboard::get_event_order_topic(strategy_id);
    let (handler, event_messages): (_, TypedMessageSavingHandler<OrderEventAny>) =
        get_typed_message_saving_handler(None);
    msgbus::subscribe_order_events(topic.into(), handler.clone(), None);

    let report = create_order_report(
        None,
        VenueOrderId::from("V-EXT-PUBLISH-DIRECT"),
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from("0"),
    );
    ctx.exec_engine
        .borrow_mut()
        .reconcile_order_status_report(&report);

    msgbus::unsubscribe_order_events(topic.into(), &handler);

    let messages = event_messages.get_messages();
    assert_eq!(messages.len(), 2);

    match &messages[0] {
        OrderEventAny::Initialized(initialized) => {
            assert_eq!(
                initialized.client_order_id,
                ClientOrderId::from("V-EXT-PUBLISH-DIRECT")
            );
            assert_eq!(initialized.strategy_id, strategy_id);
            assert!(initialized.reconciliation);
        }
        event => panic!("Expected OrderInitialized event, was {event:?}"),
    }

    assert!(matches!(messages[1], OrderEventAny::Accepted(_)));
}

#[rstest]
fn test_order_fill_replay_propagates_position_id_to_oto_contingent_order() {
    let ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = test_instrument_id();
    let primary_id = ClientOrderId::from("O-OTO-PRIMARY");
    let contingent_id = ClientOrderId::from("O-OTO-CONTINGENT");
    let position_id = PositionId::from("P-OTO-001");

    ctx.add_instrument(instrument.clone());

    let mut primary_order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(primary_id)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.0"))
        .price(Price::from("3000.00"))
        .contingency_type(ContingencyType::Oto)
        .linked_order_ids(vec![contingent_id])
        .build();
    apply_submitted_and_accepted(&mut primary_order, VenueOrderId::from("V-OTO-PRIMARY"));

    let mut contingent_order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(contingent_id)
        .instrument_id(instrument_id)
        .side(OrderSide::Sell)
        .quantity(Quantity::from("1.0"))
        .price(Price::from("3100.00"))
        .build();
    apply_submitted_and_accepted(
        &mut contingent_order,
        VenueOrderId::from("V-OTO-CONTINGENT"),
    );

    let strategy_id = primary_order.strategy_id();
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(strategy_id, OmsType::Hedging);
    ctx.add_order(primary_order.clone());
    ctx.add_order(contingent_order);

    let fill = TestOrderEventStubs::filled(
        &primary_order,
        &instrument,
        Some(TradeId::from("T-OTO-001")),
        Some(position_id),
        Some(Price::from("3000.00")),
        Some(Quantity::from("1.0")),
        Some(LiquiditySide::Taker),
        Some(Money::from("0 USDT")),
        Some(UnixNanos::from(2_000_000)),
        Some(test_account_id()),
    );
    ctx.exec_engine.borrow_mut().process(&fill);

    let cache = ctx.cache.borrow();
    let primary_after = cache.order(&primary_id).unwrap();
    let contingent_after = cache.order(&contingent_id).unwrap();

    assert_eq!(primary_after.position_id(), Some(position_id));
    assert_eq!(contingent_after.position_id(), Some(position_id));
    assert_eq!(cache.position_id(&contingent_id), Some(&position_id));
}

#[rstest]
fn test_order_fill_replay_propagates_position_id_to_exec_spawn_primary_order() {
    let ctx = TestContext::new();
    let instrument = test_instrument();
    let instrument_id = test_instrument_id();
    let primary_id = ClientOrderId::from("O-SPAWN-PRIMARY");
    let spawned_id = ClientOrderId::from("O-SPAWN-CHILD");
    let position_id = PositionId::from("P-SPAWN-001");

    ctx.add_instrument(instrument.clone());

    let mut primary_order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(primary_id)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.0"))
        .price(Price::from("3000.00"))
        .build();
    apply_submitted_and_accepted(&mut primary_order, VenueOrderId::from("V-SPAWN-PRIMARY"));

    let mut spawned_order = OrderTestBuilder::new(OrderType::Limit)
        .client_order_id(spawned_id)
        .instrument_id(instrument_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1.0"))
        .price(Price::from("3000.00"))
        .exec_algorithm_id(ExecAlgorithmId::from("ALG-SPAWN"))
        .exec_spawn_id(primary_id)
        .build();
    apply_submitted_and_accepted(&mut spawned_order, VenueOrderId::from("V-SPAWN-CHILD"));

    let strategy_id = spawned_order.strategy_id();
    ctx.exec_engine
        .borrow_mut()
        .register_oms_type(strategy_id, OmsType::Hedging);
    ctx.add_order(primary_order);
    ctx.add_order(spawned_order.clone());

    let fill = TestOrderEventStubs::filled(
        &spawned_order,
        &instrument,
        Some(TradeId::from("T-SPAWN-001")),
        Some(position_id),
        Some(Price::from("3000.00")),
        Some(Quantity::from("1.0")),
        Some(LiquiditySide::Taker),
        Some(Money::from("0 USDT")),
        Some(UnixNanos::from(2_000_000)),
        Some(test_account_id()),
    );
    ctx.exec_engine.borrow_mut().process(&fill);

    let cache = ctx.cache.borrow();
    let primary_after = cache.order(&primary_id).unwrap();
    let spawned_after = cache.order(&spawned_id).unwrap();

    assert_eq!(primary_after.position_id(), Some(position_id));
    assert_eq!(spawned_after.position_id(), Some(position_id));
    assert_eq!(cache.position_id(&primary_id), Some(&position_id));
}

#[rstest]
#[case(OrderStatus::Canceled)]
#[case(OrderStatus::Rejected)]
#[case(OrderStatus::Expired)]
fn test_runtime_terminal_unknown_price_without_executions(
    #[case] status: OrderStatus,
    #[values(false, true)] zero_fill: bool,
    #[values(false, true)] snapshot: bool,
) {
    let ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-UNKNOWN-EMPTY-BUNDLE");
    let venue_order_id = VenueOrderId::from("V-UNKNOWN-EMPTY-BUNDLE");
    ctx.add_instrument(test_instrument());
    let mut report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        status,
        Quantity::from("0.1"),
        Quantity::zero(1),
    );
    report.price = None;

    let fills = if zero_fill {
        vec![create_fill_report(
            client_order_id,
            venue_order_id,
            instrument_id,
            TradeId::from("T-ZERO-UNKNOWN"),
            "0.0",
        )]
    } else {
        Vec::new()
    };

    let (order_handler, order_saver) = get_any_saving_handler::<OrderStatusReport>(None);
    let (fill_handler, fill_saver) = get_any_saving_handler::<FillReport>(None);
    let order_topic = MessagingSwitchboard::reconciliation_raw_order_status_report_topic().into();
    let fill_topic = MessagingSwitchboard::reconciliation_raw_fill_report_topic().into();
    msgbus::subscribe_any(order_topic, order_handler.clone(), None);
    msgbus::subscribe_any(fill_topic, fill_handler.clone(), None);
    let expected_fills = fills.clone();

    if snapshot {
        ctx.exec_engine
            .borrow_mut()
            .reconcile_execution_mass_status(&create_mass_status(vec![report.clone()], fills));
    } else {
        ctx.exec_engine
            .borrow_mut()
            .reconcile_execution_report(&ExecutionReport::OrderWithFills(
                Box::new(report.clone()),
                fills,
            ));
    }

    msgbus::unsubscribe_any(order_topic, &order_handler);
    msgbus::unsubscribe_any(fill_topic, &fill_handler);
    let order = ctx.get_order(&client_order_id).unwrap();
    let events = order.events();

    let expected = if status == OrderStatus::Rejected {
        2
    } else {
        3
    };

    assert_eq!(order_saver.get_messages(), vec![report.clone()]);
    assert_eq!(fill_saver.get_messages(), expected_fills);
    assert_eq!(events.len(), expected);
    assert_eq!(events.last().unwrap().ts_event(), report.ts_last);

    if status == OrderStatus::Rejected {
        assert!(matches!(events[0], OrderEventAny::Initialized(_)));
        assert!(matches!(events[1], OrderEventAny::Rejected(_)));
    }

    assert_eq!(order.status(), status);
    assert_eq!(order.price(), Some(Price::zero(0)));
    assert_eq!(order.quantity(), Quantity::from("0.1"));
    assert_eq!(order.filled_qty(), Quantity::zero(1));
    assert_eq!(order.avg_px(), None);
    assert_eq!(order.slippage(), None);
    assert_eq!(
        order.tags(),
        Some([Ustr::from(RECONCILIATION_PRICE_UNKNOWN_TAG)].as_slice())
    );
}

#[rstest]
fn test_priced_rejected_bundle_keeps_existing_event_history() {
    let ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-PRICED-REJECTED-BUNDLE");
    ctx.add_instrument(test_instrument());
    let report = create_order_report(
        Some(client_order_id),
        VenueOrderId::from("V-PRICED-REJECTED-BUNDLE"),
        instrument_id,
        OrderStatus::Rejected,
        Quantity::from("0.1"),
        Quantity::zero(1),
    );
    let price = report.price.unwrap();
    ctx.exec_engine
        .borrow_mut()
        .reconcile_execution_report(&ExecutionReport::OrderWithFills(
            Box::new(report),
            Vec::new(),
        ));
    let order = ctx.get_order(&client_order_id).unwrap();

    assert!(matches!(
        order.events().as_slice(),
        [
            OrderEventAny::Initialized(_),
            OrderEventAny::Accepted(_),
            OrderEventAny::Rejected(_)
        ]
    ));
    assert_eq!(order.status(), OrderStatus::Rejected);
    assert_eq!(order.price(), Some(price));
}

#[rstest]
fn test_empty_bundle_preserves_external_stop_limit_trigger() {
    let ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-EXTERNAL-TRIGGERED");
    ctx.add_instrument(test_instrument());
    let mut report = create_order_report(
        Some(client_order_id),
        VenueOrderId::from("V-EXTERNAL-TRIGGERED"),
        instrument_id,
        OrderStatus::Triggered,
        Quantity::from("0.1"),
        Quantity::zero(1),
    );
    report.order_type = OrderType::StopLimit;
    report.trigger_price = Some(Price::from("2999.00"));
    report.trigger_type = Some(TriggerType::LastPrice);
    report.ts_triggered = Some(UnixNanos::from(2_000_000));
    report.ts_last = UnixNanos::from(3_000_000);
    ctx.exec_engine
        .borrow_mut()
        .reconcile_execution_report(&ExecutionReport::OrderWithFills(
            Box::new(report.clone()),
            Vec::new(),
        ));
    let order = ctx.get_order(&client_order_id).unwrap();
    let events = order.events();

    assert_eq!(events.len(), 3);
    assert!(matches!(events[0], OrderEventAny::Initialized(_)));
    assert!(matches!(events[1], OrderEventAny::Accepted(_)));
    assert!(matches!(events[2], OrderEventAny::Triggered(_)));
    assert_eq!(order.status(), OrderStatus::Triggered);
    assert_eq!(order.price(), report.price);
    assert_eq!(order.quantity(), report.quantity);
    assert_eq!(order.filled_qty(), Quantity::zero(1));

    let OrderAny::StopLimit(order) = order else {
        panic!("Expected StopLimit order");
    };

    assert!(order.is_triggered);
    assert_eq!(order.ts_triggered, report.ts_triggered);
}

#[rstest]
fn test_empty_terminal_bundle_preserves_size_before_authoritative_fill() {
    let ctx = TestContext::new();
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-EMPTY-AMEND-FILL");
    let venue_order_id = VenueOrderId::from("V-EMPTY-AMEND-FILL");
    ctx.add_instrument(test_instrument());
    ctx.add_margin_account(test_account_id());
    ctx.require_authoritative_fills();
    let order = create_accepted_order(
        "O-EMPTY-AMEND-FILL",
        instrument_id,
        OrderSide::Buy,
        "1.0",
        "3000.00",
        venue_order_id,
    );
    ctx.add_order(order);
    let report = create_order_report(
        Some(client_order_id),
        venue_order_id,
        instrument_id,
        OrderStatus::Canceled,
        Quantity::from("2.0"),
        Quantity::from("1.5"),
    );
    ctx.exec_engine
        .borrow_mut()
        .reconcile_execution_report(&ExecutionReport::OrderWithFills(
            Box::new(report.clone()),
            Vec::new(),
        ));
    let amended = ctx.get_order(&client_order_id).unwrap();
    let fill = create_fill_report(
        client_order_id,
        venue_order_id,
        instrument_id,
        TradeId::from("T-EMPTY-AMEND-FILL"),
        "1.5",
    );
    ctx.exec_engine.borrow_mut().reconcile_fill_report(&fill);
    let filled = ctx.get_order(&client_order_id).unwrap();
    ctx.exec_engine
        .borrow_mut()
        .reconcile_execution_report(&ExecutionReport::OrderWithFills(
            Box::new(report),
            Vec::new(),
        ));
    let closed = ctx.get_order(&client_order_id).unwrap();
    let cache = ctx.cache.borrow();
    let positions = cache.positions_open(None, Some(&instrument_id), None, None, None);

    assert_eq!(amended.status(), OrderStatus::Accepted);
    assert_eq!(amended.quantity(), Quantity::from("2.0"));
    assert_eq!(amended.filled_qty(), Quantity::zero(1));
    assert_eq!(filled.status(), OrderStatus::PartiallyFilled);
    assert_eq!(filled.quantity(), Quantity::from("2.0"));
    assert_eq!(filled.filled_qty(), Quantity::from("1.5"));
    assert_eq!(filled.leaves_qty(), Quantity::from("0.5"));
    assert_eq!(filled.avg_px(), Some(dec!(3000.00)));
    assert_eq!(closed.status(), OrderStatus::Canceled);
    assert_eq!(closed.quantity(), Quantity::from("2.0"));
    assert_eq!(closed.filled_qty(), Quantity::from("1.5"));
    assert_eq!(closed.price(), Some(Price::from("3000.00")));
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].quantity, Quantity::from("1.5"));
}

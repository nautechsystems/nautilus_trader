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
#[case::opening_only_keeps_cached_average(
    AvgPxReconciliation::OpeningOnly,
    None,
    dec!(65000),
    "55000.00"
)]
#[case::coarse_average_keeps_cached_average(AvgPxReconciliation::Match, Some(0), dec!(54994), "55000.00")]
#[case::match_solves_weighted_average(AvgPxReconciliation::Match, None, dec!(65000), "75000.00")]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_position_check_prices_increase_from_report_metadata(
    #[case] avg_px_reconciliation: AvgPxReconciliation,
    #[case] avg_px_precision: Option<u8>,
    #[case] report_avg_px: Decimal,
    #[case] expected_px: &str,
) {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = fifo_test_instrument();
    let instrument_id = instrument.id();
    let position = create_fifo_divergent_position(&instrument, OrderSide::Buy);
    let mut venue_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("2.000"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(report_avg_px),
    )
    .with_avg_px_open_reconciliation(avg_px_reconciliation);
    venue_report.avg_px_open_precision = avg_px_precision;
    ctx.add_instrument(instrument);
    ctx.add_position(&position);

    let mock_client = MockPositionExecutionClient::new(vec![], vec![venue_report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert_eq!(
        filled_events(&events),
        vec![(
            OrderSide::Buy,
            Quantity::from("1.000"),
            Price::from(expected_px)
        )]
    );
}

fn close_test_long_position(
    mut position: Position,
    instrument: &InstrumentAny,
    qty: &str,
    trade_id: TradeId,
) -> Position {
    let close_order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from(qty))
        .build();
    let close_fill = TestOrderEventStubs::filled(
        &close_order,
        instrument,
        Some(trade_id),
        Some(position.id),
        Some(Price::from("3000.00")),
        Some(Quantity::from(qty)),
        None,
        None,
        None,
        Some(position.account_id),
    );
    let close_filled: OrderFilled = close_fill.into();
    position.apply(&close_filled);
    position
}

#[rstest]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_cross_zero_unbuildable_open_leg_has_no_side_effects() {
    let config = ExecutionManagerConfig {
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let position_id = PositionId::from("P-CROSS-ZERO-ATOMIC");
    let position = create_test_position(&instrument, position_id, OrderSide::Buy, "5.0", "3000.00");
    let strategy_id = StrategyId::from("CROSS-ZERO-ATOMIC");
    let venue_ts_last = UnixNanos::from(1_000_000);
    ctx.add_instrument(instrument.clone());
    ctx.add_position(&position);
    ctx.manager
        .claim_external_orders(instrument_id, strategy_id)
        .unwrap();

    let topic = switchboard::get_event_order_topic(strategy_id);
    let (handler, event_messages): (_, TypedMessageSavingHandler<OrderEventAny>) =
        get_typed_message_saving_handler(None);
    msgbus::subscribe_order_events(topic.into(), handler.clone(), None);

    let mut report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Short,
        Quantity::from("3.0"),
        venue_ts_last,
        venue_ts_last,
        None,
        None,
        Some(dec!(3100.00)),
    );

    report.signed_decimal_qty = -dec!(10000000000000000000);
    let client = MockPositionExecutionClient::new(vec![], vec![report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&client];

    let order_count_before = ctx
        .cache
        .borrow()
        .orders_total_count(None, None, None, None, None);
    let events = ctx.manager.check_positions_consistency(&clients).await;

    msgbus::unsubscribe_order_events(topic.into(), &handler);

    let close_venue_order_id = create_position_reconciliation_venue_order_id(
        test_account_id(),
        instrument_id,
        OrderSide::Sell,
        OrderType::Market,
        Quantity::from("5.0"),
        Price::from_decimal_dp(dec!(3000.00), instrument.price_precision()).ok(),
        None,
        Some("CLOSE"),
        venue_ts_last,
    );
    let close_client_order_id = ClientOrderId::from(close_venue_order_id.as_str());
    let cache = ctx.cache.borrow();
    let cached_qty = cache
        .position(&position_id)
        .expect("cached position should remain present")
        .signed_decimal_qty();

    assert!(events.is_empty());
    assert_eq!(
        cache.orders_total_count(None, None, None, None, None),
        order_count_before,
        "an unbuildable open leg must not cache the close order",
    );
    assert!(
        cache.client_order_id(&close_venue_order_id).is_none(),
        "an unbuildable open leg must not add the venue-to-client order index",
    );
    assert!(
        cache.venue_order_id(&close_client_order_id).is_none(),
        "an unbuildable open leg must not add the client-to-venue order index",
    );
    assert_eq!(cached_qty, dec!(5.0));
    assert!(
        event_messages.get_messages().is_empty(),
        "an unbuildable open leg must not publish any order event",
    );
}

#[rstest]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_cross_zero_unbuildable_open_leg_retries_without_poisoning_order_id() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let key = (instrument_id, test_account_id());
    let position = create_test_position(
        &instrument,
        PositionId::from("P-CROSS-ZERO-RETRY"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
    );
    let venue_ts_last = UnixNanos::from(1_000_000);
    ctx.add_instrument(instrument);
    ctx.add_position(&position);

    let mut unbuildable_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Short,
        Quantity::from("3.0"),
        venue_ts_last,
        venue_ts_last,
        None,
        None,
        Some(dec!(3100.00)),
    );

    unbuildable_report.signed_decimal_qty = -dec!(10000000000000000000);
    let unbuildable_client = MockPositionExecutionClient::new(vec![], vec![unbuildable_report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&unbuildable_client];

    let first_events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(first_events.is_empty());
    assert_eq!(ctx.manager.position_recon_retry_count(&key), 1);

    let valid_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Short,
        Quantity::from("3.0"),
        venue_ts_last,
        venue_ts_last,
        None,
        None,
        Some(dec!(3100.00)),
    );

    let valid_client = MockPositionExecutionClient::new(vec![], vec![valid_report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&valid_client];

    let second_events = ctx.manager.check_positions_consistency(&clients).await;

    let fills: Vec<_> = second_events
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => Some(fill),
            _ => None,
        })
        .collect();

    assert_eq!(ctx.manager.position_recon_retry_count(&key), 0);
    assert_eq!(
        fills.len(),
        2,
        "the retry must rebuild both cross-zero legs"
    );
    assert_eq!(fills[0].order_side, OrderSide::Sell);
    assert_eq!(fills[0].last_qty, Quantity::from("5.0"));
    assert_eq!(fills[1].order_side, OrderSide::Sell);
    assert_eq!(fills[1].last_qty, Quantity::from("3.0"));

    for event in &second_events {
        ctx.exec_engine.borrow_mut().process(event);
    }

    let cached_signed_qty = ctx
        .cache
        .borrow()
        .positions_open(
            None,
            Some(&instrument_id),
            None,
            Some(&test_account_id()),
            None,
        )
        .iter()
        .map(|position| position.signed_decimal_qty())
        .sum::<Decimal>();
    assert_eq!(cached_signed_qty, dec!(-3.0));
}

#[tokio::test]
async fn test_position_check_reconciles_venue_only_nonflat_report() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();

    ctx.add_instrument(instrument);

    let venue_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("3.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3000.00)),
    );

    let mock_client = MockPositionExecutionClient::new(vec![], vec![venue_report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(
        events
            .iter()
            .any(|event| matches!(event, OrderEventAny::Filled(_))),
        "Expected a synthetic fill for a non-flat venue-only position report"
    );
}

#[rstest]
#[case::venue_only(false, PositionSide::Long)]
#[case::increase(true, PositionSide::Long)]
#[case::cross_zero(true, PositionSide::Short)]
#[tokio::test]
async fn test_position_check_respects_disabled_order_generation(
    #[case] has_position: bool,
    #[case] venue_side: PositionSide,
) {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        generate_missing_orders: false,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    });

    let instrument = test_instrument();
    let position_id = PositionId::from("P-GENERATION-DISABLED");
    let position = create_test_position(&instrument, position_id, OrderSide::Buy, "3.0", "3000.00");
    ctx.add_instrument(instrument);
    if has_position {
        ctx.add_position(&position);
    }

    let report = PositionStatusReport::new(
        test_account_id(),
        test_instrument_id(),
        venue_side,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3100.00)),
    );

    let client = MockPositionExecutionClient::new(Vec::new(), vec![report]);

    let events = ctx.manager.check_positions_consistency(&[&client]).await;
    let cache = ctx.cache.borrow();

    assert!(events.is_empty());
    assert!(cache.orders(None, None, None, None, None).is_empty());
    assert_eq!(
        cache.position(&position_id).as_deref(),
        has_position.then_some(&position),
    );
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(test_instrument_id(), test_account_id())),
        0
    );
}

#[tokio::test]
async fn test_position_check_updates_reported_hedge_position() {
    let config = ExecutionManagerConfig {
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let position_id = PositionId::from("P-CONTINUOUS-HEDGE-LONG");
    let position = create_test_position(&instrument, position_id, OrderSide::Buy, "5.0", "3000.00");

    let report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("7.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(position_id),
        Some(dec!(3100.00)),
    );

    ctx.add_instrument(instrument);
    ctx.add_position(&position);
    let client = MockPositionExecutionClient::new(vec![], vec![report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    let fill = events
        .iter()
        .find_map(|event| match event {
            OrderEventAny::Filled(fill) => Some(fill),
            _ => None,
        })
        .expect("reconciliation fill is emitted");

    assert_eq!(fill.position_id, Some(position_id));

    for event in &events {
        ctx.exec_engine.borrow_mut().process(event);
    }

    let cache = ctx.cache.borrow();
    let positions = cache.positions_open(
        None,
        Some(&instrument_id),
        None,
        Some(&test_account_id()),
        None,
    );
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].id, position_id);
    assert_eq!(positions[0].signed_decimal_qty(), dec!(7.0));
}

#[tokio::test]
async fn test_position_check_cross_zero_preserves_both_hedge_position_ids() {
    let config = ExecutionManagerConfig {
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let close_position_id = PositionId::from("P-CONTINUOUS-HEDGE-LONG");
    let open_position_id = PositionId::from("P-CONTINUOUS-HEDGE-SHORT");
    let position = create_test_position(
        &instrument,
        close_position_id,
        OrderSide::Buy,
        "5.0",
        "3000.00",
    );

    let report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Short,
        Quantity::from("3.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(open_position_id),
        Some(dec!(3100.00)),
    );

    ctx.add_instrument(instrument);
    ctx.add_position(&position);
    let client = MockPositionExecutionClient::new(vec![], vec![report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    let fills = events
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => Some(fill),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(fills.len(), 2);
    assert_eq!(fills[0].position_id, Some(close_position_id));
    assert_eq!(fills[1].position_id, Some(open_position_id));

    for event in &events {
        ctx.exec_engine.borrow_mut().process(event);
    }

    let cache = ctx.cache.borrow();
    assert!(
        cache
            .position(&close_position_id)
            .is_some_and(|position| position.is_closed())
    );
    assert_eq!(
        cache
            .position(&open_position_id)
            .expect("short hedge position is created")
            .signed_decimal_qty(),
        dec!(-3.0),
    );
}

#[rstest]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_position_check_rereads_position_closed_during_request() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let position = create_test_position(
        &instrument,
        PositionId::from("P-CLOSE-DURING-REQUEST"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
    );
    let closed_position = close_test_long_position(
        position.clone(),
        &instrument,
        "5.0",
        TradeId::from("T-CLOSE-DURING-REQUEST"),
    );
    ctx.add_instrument(instrument);
    ctx.add_position(&position);

    let mock_client = MockPositionExecutionClient::new(vec![], vec![])
        .with_position_request_mutation(PositionRequestMutation::Update {
            cache: ctx.cache.clone(),
            position: closed_position,
        });

    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;
    for event in &events {
        ctx.exec_engine.borrow_mut().process(event);
    }

    let open_positions = ctx
        .cache
        .borrow()
        .positions_open(None, Some(&test_instrument_id()), None, None, None)
        .len();
    assert!(events.is_empty());
    assert_eq!(
        open_positions, 0,
        "a synthetic sell from the stale long would open a real short",
    );
}

#[rstest]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_position_check_rereads_position_opened_during_request() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let position = create_test_position(
        &instrument,
        PositionId::from("P-OPEN-DURING-REQUEST"),
        OrderSide::Buy,
        "3.0",
        "3000.00",
    );

    let venue_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("3.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3000.00)),
    );

    ctx.add_instrument(instrument);

    let mock_client = MockPositionExecutionClient::new(vec![], vec![venue_report])
        .with_position_request_mutation(PositionRequestMutation::Add {
            cache: ctx.cache.clone(),
            position,
        });

    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(
        events.is_empty(),
        "the matching report must not recreate a position opened during the request",
    );
}

#[rstest]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_position_check_uses_current_avg_px_after_request() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let position_id = PositionId::from("P-AVG-PX-DURING-REQUEST");
    let stale_position =
        create_test_position(&instrument, position_id, OrderSide::Buy, "5.0", "3000.00");
    let current_position =
        create_test_position(&instrument, position_id, OrderSide::Buy, "3.0", "3100.00");

    let venue_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("6.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3200.00)),
    );

    ctx.add_instrument(instrument);
    ctx.add_position(&stale_position);

    let mock_client = MockPositionExecutionClient::new(vec![], vec![venue_report])
        .with_position_request_mutation(PositionRequestMutation::Update {
            cache: ctx.cache.clone(),
            position: current_position,
        });

    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    let fill = events
        .iter()
        .find_map(|event| match event {
            OrderEventAny::Filled(fill) => Some(fill),
            _ => None,
        })
        .expect("expected a reconciliation fill");

    assert_eq!(fill.last_qty, Quantity::from("3.0"));
    assert_eq!(fill.last_px, Price::from("3300.00"));
}

// Cached long 1 at 10,000 and venue long 2 at 12,000: weighted by base value, the venue average
// needs one contract at 15,000.
#[rstest]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_position_check_inverse_accumulation_fill_reaches_venue_avg_px() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument2();
    let instrument_id = instrument.id();
    let position = create_test_position(
        &instrument,
        PositionId::from("P-INVERSE-ADD"),
        OrderSide::Buy,
        "1",
        "10000.0",
    );

    let venue_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("2"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(12000.0)),
    );

    ctx.add_instrument(instrument);
    ctx.add_position(&position);

    let mock_client = MockPositionExecutionClient::new(vec![], vec![venue_report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    let fill = events
        .iter()
        .find_map(|event| match event {
            OrderEventAny::Filled(fill) => Some(fill),
            _ => None,
        })
        .expect("expected a reconciliation fill");

    assert_eq!(fill.last_qty, Quantity::from("1"));
    assert_eq!(fill.last_px, Price::from("15000.0"));
}

#[rstest]
#[case::no_positive_price("1", "10000.0", "2", dec!(25000), true, None)]
#[case::above_max("100", "50000.0", "101", dec!(50499), true, None)]
#[case::max_matches("100", "50000.0", "101", dec!(50475), true, Some(Price::from("1000000.0")))]
#[case::max_matches_without_exact_price("1000", "50000.0", "1001", dec!(50051), true, Some(Price::from("1000000.0")))]
#[case::below_min("1000000", "50000.0", "1000001", dec!(47000), true, None)]
#[case::without_price_cap("1000000", "50000.0", "1000010", dec!(50001), false, Some(Price::max(1)))]
#[case::unrepresentable_size("1.4", "10000.0", "2", dec!(12000), true, None)]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_position_check_inverse_accumulation_checks_price_bounds(
    #[case] cached_qty: &str,
    #[case] cached_px: &str,
    #[case] venue_qty: &str,
    #[case] venue_avg_px: Decimal,
    #[case] has_price_cap: bool,
    #[case] expected_px: Option<Price>,
) {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let mut instrument = btcusd_bybit();
    instrument.max_price = has_price_cap.then(|| Price::from("1000000.0"));
    instrument.min_price = Some(Price::from("1.0"));
    let instrument = InstrumentAny::CryptoPerpetual(instrument);
    let instrument_id = instrument.id();
    let position_id = PositionId::from("P-INVERSE-UNREACHABLE");
    let position = create_test_position(
        &instrument,
        position_id,
        OrderSide::Buy,
        cached_qty,
        cached_px,
    );

    let venue_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from(venue_qty),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(venue_avg_px),
    );

    ctx.add_instrument(instrument);
    ctx.add_position(&position);

    let mock_client = MockPositionExecutionClient::new(vec![], vec![venue_report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    let fills: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => Some((fill.last_qty, fill.last_px)),
            _ => None,
        })
        .collect();

    let expected = expected_px.map(|px| {
        let qty = Quantity::from(venue_qty).as_decimal() - Quantity::from(cached_qty).as_decimal();
        (Quantity::from_decimal_dp(qty, 0).unwrap(), px)
    });

    assert_eq!(fills, expected.as_slice());
    let cache = ctx.cache.borrow();
    let position = cache.position(&position_id).expect("position must remain");
    assert_eq!(
        position.signed_decimal_qty(),
        Quantity::from(cached_qty).as_decimal()
    );
    assert_eq!(position.avg_px_open, Price::from(cached_px).as_decimal());
}

#[rstest]
#[case::reachable("50", OrderSide::Buy, "150", dec!(20000), Some(Price::from("40000.0")))]
#[case::unreachable("50", OrderSide::Buy, "150", dec!(25000), None)]
#[case::opposing_legs("100", OrderSide::Sell, "51", dec!(38000), None)]
#[case::opposing_legs_net_flat("50", OrderSide::Sell, "1", dec!(38000), None)]
#[tokio::test]
async fn test_position_check_inverse_accumulation_uses_all_cached_open_costs(
    #[case] first_qty: &str,
    #[case] second_side: OrderSide,
    #[case] venue_qty: &str,
    #[case] venue_avg_px: Decimal,
    #[case] expected_fill_px: Option<Price>,
) {
    let config = ExecutionManagerConfig {
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument2();
    let instrument_id = instrument.id();

    let (first_px, second_px) = if second_side == OrderSide::Sell {
        ("50000.0", "25000.0")
    } else {
        ("10000.0", "40000.0")
    };

    let first = create_test_position(
        &instrument,
        PositionId::from("P-INVERSE-FIRST"),
        OrderSide::Buy,
        first_qty,
        first_px,
    );
    let second = create_test_position(
        &instrument,
        PositionId::from("P-INVERSE-SECOND"),
        second_side,
        "50",
        second_px,
    );
    ctx.add_instrument(instrument);
    ctx.add_position(&first);
    ctx.add_position(&second);

    let venue_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from(venue_qty),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(venue_avg_px),
    );

    let mock_client = MockPositionExecutionClient::new(vec![], vec![venue_report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    let fills: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => Some((fill.last_qty, fill.last_px)),
            _ => None,
        })
        .collect();

    let expected_fill = expected_fill_px.map(|px| (Quantity::from("50"), px));
    assert_eq!(fills, expected_fill.as_slice());
}

#[tokio::test]
async fn test_reconcile_hedge_unreachable_inverse_avg_px_books_no_fill() {
    let config = ExecutionManagerConfig {
        generate_missing_orders: true,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument2();
    let instrument_id = instrument.id();
    let position_id = PositionId::from("P-HEDGE-INVERSE-UNREACHABLE");
    let position = create_test_position(&instrument, position_id, OrderSide::Buy, "1", "10000.0");
    ctx.add_instrument(instrument);
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
        PositionSide::Long,
        Quantity::from("2"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        Some(position_id),
        Some(dec!(25000.0)),
    );

    mass_status.add_position_reports(vec![position_report]);

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert!(
        !result
            .events
            .iter()
            .any(|event| matches!(event, OrderEventAny::Filled(_))),
        "an unreachable inverse average must not book a fill, was {:?}",
        result.events,
    );
    let cache = ctx.cache.borrow();
    let position = cache.position(&position_id).expect("position must remain");
    assert_eq!(position.signed_decimal_qty(), dec!(1));
    assert_eq!(position.avg_px_open, dec!(10000.0));
}

#[rstest]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_position_check_preserves_retry_for_uncovered_position_opened_during_request() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let key = (instrument_id, test_account_id());
    let old_position = create_test_position(
        &instrument,
        PositionId::from("P-RETRY-OLD"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
    );
    ctx.add_position(&old_position);

    let initial_client = MockPositionExecutionClient::new(vec![], vec![]);
    let clients: Vec<&dyn ExecutionClient> = vec![&initial_client];
    assert!(
        ctx.manager
            .check_positions_consistency(&clients)
            .await
            .is_empty()
    );
    assert_eq!(ctx.manager.position_recon_retry_count(&key), 1);

    let closed_position = close_test_long_position(
        old_position,
        &instrument,
        "5.0",
        TradeId::from("T-RETRY-CLOSE"),
    );
    ctx.cache
        .borrow_mut()
        .update_position(&closed_position)
        .unwrap();
    ctx.add_instrument(instrument.clone());

    let new_position = create_test_position(
        &instrument,
        PositionId::from("P-RETRY-NEW"),
        OrderSide::Buy,
        "3.0",
        "3100.00",
    );

    let venue_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("4.0"),
        UnixNanos::from(2_000_000),
        UnixNanos::from(2_000_000),
        None,
        None,
        Some(dec!(3200.00)),
    );

    let request_client = MockPositionExecutionClient::new(vec![], vec![venue_report])
        .with_position_request_mutation(PositionRequestMutation::Add {
            cache: ctx.cache.clone(),
            position: new_position,
        });

    let clients: Vec<&dyn ExecutionClient> = vec![&request_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(events.is_empty());
    assert_eq!(
        ctx.manager.position_recon_retry_count(&key),
        1,
        "the uncovered key must be deferred and remain active for retry pruning",
    );
}

#[tokio::test]
async fn test_position_check_retries_stops_after_max() {
    // When instrument is not in cache, check_position_discrepancy returns None
    // (can't generate fills), so retries should increment until exhausted
    let config = ExecutionManagerConfig {
        position_check_retries: 2,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let position_id = PositionId::from("P-001");

    let position = create_test_position(&instrument, position_id, OrderSide::Buy, "5.0", "3000.00");

    // Add position to cache but NOT the instrument - forces reconciliation to
    // return None on the cache.instrument() lookup
    ctx.add_position(&position);

    let mock_client = MockExecutionClient::new(vec![]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    // First attempt: detects discrepancy, can't reconcile, retry count -> 1
    let events = ctx.manager.check_positions_consistency(&clients).await;
    assert!(events.is_empty());

    // Second attempt: retry count -> 2 (= max), logs error
    let events = ctx.manager.check_positions_consistency(&clients).await;
    assert!(events.is_empty());

    // Third attempt: retries exhausted, silently skipped
    let events = ctx.manager.check_positions_consistency(&clients).await;
    assert!(events.is_empty());
}

#[tokio::test]
async fn test_position_check_retries_clears_when_discrepancy_resolves() {
    // When reconciliation succeeds (generates events), the retry counter resets
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let position_id = PositionId::from("P-001");

    let position = create_test_position(&instrument, position_id, OrderSide::Buy, "5.0", "3000.00");

    // First: add position without instrument to force a failed retry
    ctx.add_position(&position);
    let mock_client = MockExecutionClient::new(vec![]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;
    assert!(events.is_empty()); // Failed, retry count = 1

    // Now add instrument - reconciliation can succeed and generate events
    ctx.add_instrument(instrument.clone());

    let events = ctx.manager.check_positions_consistency(&clients).await;
    assert!(
        !events.is_empty(),
        "Expected reconciliation events when instrument is available"
    );
}

#[tokio::test]
async fn test_position_check_stale_retries_pruned_when_position_closed() {
    // When a position is closed and no longer reported by venue, the maxed
    // retry counter should be pruned so future discrepancies aren't suppressed
    let config = ExecutionManagerConfig {
        position_check_retries: 1,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let position_id = PositionId::from("P-001");

    ctx.add_instrument(instrument.clone());
    let position = create_test_position(&instrument, position_id, OrderSide::Buy, "5.0", "3000.00");
    ctx.add_position(&position);

    let mock_client = MockExecutionClient::new(vec![]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    // First call: reconciliation succeeds (generates events to match venue=flat)
    let events = ctx.manager.check_positions_consistency(&clients).await;
    assert!(!events.is_empty());

    // Simulate that reconciliation didn't actually fix it (position still open)
    // by not applying the events. Instead, directly max out the retry counter
    // by calling again - the position is still discrepant
    ctx.manager.check_positions_consistency(&clients).await;

    // Now close the position so it disappears from open positions
    let close_order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_id)
        .side(OrderSide::Sell)
        .quantity(Quantity::from("5.0"))
        .build();
    let close_fill = TestOrderEventStubs::filled(
        &close_order,
        &instrument,
        Some(TradeId::new("T-CLOSE-001")),
        Some(position_id),
        Some(Price::from("3000.00")),
        Some(Quantity::from("5.0")),
        None,
        None,
        None,
        Some(test_account_id()),
    );
    let close_filled: OrderFilled = close_fill.into();
    let mut pos = position;
    pos.apply(&close_filled);
    ctx.cache.borrow_mut().update_position(&pos).unwrap();

    // Run consistency check with no open positions - should prune stale counter
    ctx.manager.check_positions_consistency(&clients).await;

    // Create a new position for the same instrument
    let position2 = create_test_position(
        &instrument,
        PositionId::from("P-002"),
        OrderSide::Buy,
        "3.0",
        "3100.00",
    );
    ctx.add_position(&position2);

    // Should produce events (counter was pruned, not suppressed)
    let events = ctx.manager.check_positions_consistency(&clients).await;
    assert!(
        !events.is_empty(),
        "Expected events: stale retry counter should have been pruned"
    );
}

#[tokio::test]
async fn test_position_check_uses_client_tolerance_for_missing_dust_report() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let position = create_test_position(
        &instrument,
        PositionId::from("P-DUST-MISSING"),
        OrderSide::Buy,
        "0.008007",
        "3000.00",
    );
    ctx.add_instrument(instrument);
    ctx.add_position(&position);

    let mock_client = MockPositionExecutionClient::new(vec![], vec![])
        .with_position_reconciliation_tolerance(dec!(0.009999));
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(events.is_empty());
}

#[tokio::test]
async fn test_position_check_uses_client_tolerance_for_observed_smoke_difference() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let position = create_test_position(
        &instrument,
        PositionId::from("P-SMOKE-DIFFERENCE"),
        OrderSide::Buy,
        "5.202897",
        "3000.00",
    );

    // The venue report includes a pre-existing 0.005103-share balance in addition to this order.
    let venue_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.208000"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3000.00)),
    );

    ctx.add_instrument(instrument);
    ctx.add_position(&position);

    let mock_client = MockPositionExecutionClient::new(vec![], vec![venue_report])
        .with_position_reconciliation_tolerance(dec!(0.009999));
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(events.is_empty());
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_position_check_uses_routing_client_tolerance() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let account_id = AccountId::from("IB-ROUTING-CHECK");
    let position = create_test_position_for_account(
        &instrument,
        PositionId::from("P-ROUTING-TOLERANCE-CHECK"),
        OrderSide::Buy,
        "5.000000",
        "3000.00",
        account_id,
    );

    let venue_report = PositionStatusReport::new(
        account_id,
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.005000"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3000.00)),
    );

    ctx.add_instrument(instrument);
    ctx.add_margin_account(account_id);
    ctx.add_position(&position);

    let routing_client = MockPositionExecutionClient::configured(
        ClientId::from("IB-ROUTING-CHECK"),
        account_id,
        Venue::from("IB"),
        IndexSet::from([instrument_id.venue]),
        false,
    )
    .with_position_reports(vec![venue_report])
    .with_position_reconciliation_tolerance(dec!(0.010000));
    let clients: Vec<&dyn ExecutionClient> = vec![&routing_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(events.is_empty());
}

#[rstest]
#[case::magnitude("5.000000", PositionSide::Long, "5.005000")]
#[case::flat_dust("0.005000", PositionSide::Flat, "0.000000")]
#[case::opposite_dust("0.003000", PositionSide::Short, "0.003000")]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_mass_status_netting_uses_routing_client_tolerance(
    #[case] cached_qty: &str,
    #[case] report_side: PositionSide,
    #[case] report_qty: &str,
) {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let account_id = AccountId::from("IB-ROUTING-MASS-STATUS");
    let position = create_test_position_for_account(
        &instrument,
        PositionId::from("P-ROUTING-TOLERANCE-MASS-STATUS"),
        OrderSide::Buy,
        cached_qty,
        "3000.00",
        account_id,
    );

    let matching_report = PositionStatusReport::new(
        account_id,
        instrument_id,
        PositionSide::Long,
        Quantity::from(cached_qty),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3000.00)),
    );

    ctx.add_instrument(instrument);
    ctx.add_margin_account(account_id);
    ctx.add_position(&position);

    let routing_client = MockPositionExecutionClient::configured(
        ClientId::from("IB-ROUTING-MASS-STATUS"),
        account_id,
        Venue::from("IB"),
        IndexSet::from([instrument_id.venue]),
        false,
    )
    .with_position_reports(vec![matching_report])
    .with_position_reconciliation_tolerance(dec!(0.010000));
    let clients: Vec<&dyn ExecutionClient> = vec![&routing_client];
    // Seed the manager's per-account tolerance state (production seeds this in the builder);
    // the matching report must not itself generate reconciliation events.
    let seed_events = ctx.manager.check_positions_consistency(&clients).await;
    assert!(seed_events.is_empty());

    let mut mass_status = ExecutionMassStatus::new(
        routing_client.client_id(),
        account_id,
        routing_client.venue(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    let drift_report = PositionStatusReport::new(
        account_id,
        instrument_id,
        report_side,
        Quantity::from(report_qty),
        UnixNanos::from(2_000_000),
        UnixNanos::from(2_000_000),
        None,
        None,
        Some(dec!(3000.00)),
    );

    mass_status.add_position_reports(vec![drift_report]);

    ctx.exec_engine
        .borrow_mut()
        .register_client(Box::new(routing_client))
        .unwrap();

    let result = ctx
        .manager
        .reconcile_execution_mass_status(&mass_status, &ctx.exec_engine);

    assert!(result.events.is_empty());
    assert!(result.unresolved_positions.is_empty());
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_routing_clients_on_same_venue_use_account_tolerances_independently() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let tolerant_account_id = AccountId::from("IB-TOLERANT");
    let strict_account_id = AccountId::from("IB-STRICT");
    let tolerant_position = create_test_position_for_account(
        &instrument,
        PositionId::from("P-ROUTING-TOLERANT"),
        OrderSide::Buy,
        "5.000000",
        "3000.00",
        tolerant_account_id,
    );
    let strict_position = create_test_position_for_account(
        &instrument,
        PositionId::from("P-ROUTING-STRICT"),
        OrderSide::Buy,
        "5.000000",
        "3000.00",
        strict_account_id,
    );

    let tolerant_report = PositionStatusReport::new(
        tolerant_account_id,
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.005000"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3000.00)),
    );

    let strict_report = PositionStatusReport::new(
        strict_account_id,
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.005000"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3000.00)),
    );

    ctx.add_instrument(instrument);
    ctx.add_margin_account(tolerant_account_id);
    ctx.add_margin_account(strict_account_id);
    ctx.add_position(&tolerant_position);
    ctx.add_position(&strict_position);

    let tolerant_client = MockPositionExecutionClient::configured(
        ClientId::from("IB-TOLERANT"),
        tolerant_account_id,
        Venue::from("IB"),
        IndexSet::from([instrument_id.venue]),
        false,
    )
    .with_position_reports(vec![tolerant_report])
    .with_position_reconciliation_tolerance(dec!(0.010000));
    let strict_client = MockPositionExecutionClient::configured(
        ClientId::from("IB-STRICT"),
        strict_account_id,
        Venue::from("IB"),
        IndexSet::from([instrument_id.venue]),
        false,
    )
    .with_position_reports(vec![strict_report])
    .with_position_reconciliation_tolerance(dec!(0.001000));
    let clients: Vec<&dyn ExecutionClient> = vec![&tolerant_client, &strict_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    let fill_account_ids = events
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => Some(fill.account_id),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(fill_account_ids, vec![strict_account_id]);
}

#[tokio::test]
async fn test_position_check_reconciles_at_client_tolerance_boundary() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let position = create_test_position(
        &instrument,
        PositionId::from("P-TOLERANCE-BOUNDARY"),
        OrderSide::Buy,
        "5.200000",
        "3000.00",
    );

    let venue_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.210000"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3000.00)),
    );

    ctx.add_instrument(instrument);
    ctx.add_position(&position);

    let mock_client = MockPositionExecutionClient::new(vec![], vec![venue_report])
        .with_position_reconciliation_tolerance(dec!(0.009999));
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(events
        .iter()
        .any(|event| matches!(event, OrderEventAny::Filled(fill) if fill.last_qty == Quantity::from("0.010000"))));
}

#[tokio::test]
async fn test_position_check_failed_client_query_skips_cached_position() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let position = create_test_position(
        &instrument,
        PositionId::from("P-FAIL-SKIP"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
    );
    ctx.add_position(&position);

    let ok_client = MockPositionExecutionClient::new(vec![], vec![]);
    let clients: Vec<&dyn ExecutionClient> = vec![&ok_client];
    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(events.is_empty());
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, test_account_id())),
        1
    );

    ctx.add_instrument(instrument);
    let failing_client = MockPositionExecutionClient::failing_position_reports();
    let clients: Vec<&dyn ExecutionClient> = vec![&failing_client];
    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(
        !events
            .iter()
            .any(|event| matches!(event, OrderEventAny::Filled(_))),
        "Expected no synthetic fill when the venue position query fails"
    );
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, test_account_id())),
        1
    );
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_position_check_failed_routing_client_leaves_exchange_retry_untouched() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let account_id = AccountId::from("IB-001");
    let position = create_test_position_for_account(
        &instrument,
        PositionId::from("P-FAILED-ROUTING"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
        account_id,
    );
    ctx.add_margin_account(account_id);
    ctx.add_position(&position);

    let routing_client = MockPositionExecutionClient::configured(
        ClientId::from("IB"),
        account_id,
        Venue::from("IB"),
        IndexSet::from([instrument_id.venue]),
        true,
    );
    let clients: Vec<&dyn ExecutionClient> = vec![&routing_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(events.is_empty());
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, account_id)),
        0,
    );
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_position_check_failed_client_does_not_suppress_healthy_account_same_venue() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let failed_account_id = AccountId::from("BINANCE-FAILED");
    let healthy_account_id = AccountId::from("BINANCE-HEALTHY");
    let failed_position = create_test_position_for_account(
        &instrument,
        PositionId::from("P-FAILED-SHARED-VENUE"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
        failed_account_id,
    );
    let healthy_position = create_test_position_for_account(
        &instrument,
        PositionId::from("P-HEALTHY-SHARED-VENUE"),
        OrderSide::Buy,
        "7.0",
        "3000.00",
        healthy_account_id,
    );
    ctx.add_margin_account(failed_account_id);
    ctx.add_margin_account(healthy_account_id);
    ctx.add_position(&failed_position);
    ctx.add_position(&healthy_position);

    let failed_client = MockPositionExecutionClient::configured(
        ClientId::from("SHARED-FAILED"),
        failed_account_id,
        test_venue(),
        IndexSet::from([instrument_id.venue]),
        true,
    );
    let healthy_client = MockPositionExecutionClient::configured(
        ClientId::from("SHARED-HEALTHY"),
        healthy_account_id,
        test_venue(),
        IndexSet::from([instrument_id.venue]),
        false,
    );
    let clients: Vec<&dyn ExecutionClient> = vec![&failed_client, &healthy_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(events.is_empty());
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, failed_account_id)),
        0,
    );
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, healthy_account_id)),
        1,
    );
}

#[tokio::test]
async fn test_position_check_aggregates_hedge_positions_before_comparing_report() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();

    ctx.add_instrument(instrument.clone());
    let pos_long = create_test_position(
        &instrument,
        PositionId::from("P-NET-LONG"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
    );
    let pos_short = create_test_position(
        &instrument,
        PositionId::from("P-NET-SHORT"),
        OrderSide::Sell,
        "3.0",
        "3100.00",
    );
    ctx.add_position(&pos_long);
    ctx.add_position(&pos_short);

    let venue_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("2.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3050.00)),
    );

    let mock_client = MockPositionExecutionClient::new(vec![], vec![venue_report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(
        !events
            .iter()
            .any(|event| matches!(event, OrderEventAny::Filled(_))),
        "Expected no synthetic fill when cached hedge positions net to the venue report"
    );
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, test_account_id())),
        0
    );
}

#[rstest]
#[case::duplicates(0, false)]
#[case::distinct_snapshots(1, true)]
#[tokio::test]
async fn test_position_check_ignores_duplicate_position_reports(
    #[case] ts_last_step: u64,
    #[case] discrepant: bool,
) {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let key = (instrument_id, test_account_id());

    ctx.add_instrument(instrument.clone());
    let position = create_test_position(
        &instrument,
        PositionId::from("P-NET"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
    );
    ctx.add_position(&position);

    let reports: Vec<PositionStatusReport> = (0..2)
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
                Some(dec!(3000.00)),
            )
        })
        .collect();

    let mock_client = MockPositionExecutionClient::new(vec![], reports.clone());
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];
    let queried_clients = IndexSet::from([mock_client.client_id()]);
    let mut check = ctx
        .manager
        .prepare_position_report_check(UUID4::new(), &clients);

    let plan = ctx.manager.plan_position_fill_reports(
        &mut check,
        &reports,
        &queried_clients,
        &IndexSet::new(),
        &clients,
    );
    let events = ctx.manager.check_positions_consistency(&clients).await;

    let discrepancy_keys = if discrepant {
        IndexSet::from([key])
    } else {
        IndexSet::new()
    };

    assert_eq!(plan.discrepancy_keys, discrepancy_keys);
    assert!(events.is_empty());
    assert_eq!(
        ctx.manager.position_recon_retry_count(&key),
        u32::from(discrepant)
    );
}

#[tokio::test]
async fn test_position_check_skips_reports_outside_reconciliation_instruments() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        reconciliation_instrument_ids: IndexSet::from([test_instrument_id()]),
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let excluded_key = (test_instrument_id2(), test_account_id());

    let report = PositionStatusReport::new(
        test_account_id(),
        test_instrument_id2(),
        PositionSide::Long,
        Quantity::from("5"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(50000.0)),
    );

    let mock_client = MockPositionExecutionClient::new(vec![], vec![report.clone()]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];
    let queried_clients = IndexSet::from([mock_client.client_id()]);
    let mut check = ctx
        .manager
        .prepare_position_report_check(UUID4::new(), &clients);

    let plan = ctx.manager.plan_position_fill_reports(
        &mut check,
        &[report],
        &queried_clients,
        &IndexSet::new(),
        &clients,
    );
    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(plan.discrepancy_keys.is_empty());
    assert!(plan.queries.is_empty());
    assert!(events.is_empty());
    assert_eq!(ctx.manager.position_recon_retry_count(&excluded_key), 0);
}

#[rstest]
#[case(false)]
#[case(true)]
#[tokio::test]
async fn test_position_check_matching_hedge_reports_is_order_invariant(
    #[case] reverse_reports: bool,
) {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();

    ctx.add_instrument(instrument.clone());
    let pos_long = create_test_position(
        &instrument,
        PositionId::from("P-MATCH-LONG"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
    );
    let pos_short = create_test_position(
        &instrument,
        PositionId::from("P-MATCH-SHORT"),
        OrderSide::Sell,
        "2.0",
        "3100.00",
    );
    ctx.add_position(&pos_long);
    ctx.add_position(&pos_short);

    let report_long = create_test_position_report(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        "5.0",
        "P-MATCH-LONG",
        dec!(3000.00),
    );
    let report_short = create_test_position_report(
        test_account_id(),
        instrument_id,
        PositionSide::Short,
        "2.0",
        "P-MATCH-SHORT",
        dec!(3100.00),
    );
    let mut reports = vec![report_long, report_short];
    if reverse_reports {
        reports.reverse();
    }

    let mock_client = MockPositionExecutionClient::new(vec![], reports);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(events.is_empty());
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, test_account_id())),
        0,
    );
}

#[tokio::test]
async fn test_position_check_equal_net_with_mismatched_hedge_legs_is_discrepant() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let pos_long = create_test_position(
        &instrument,
        PositionId::from("P-NET-MISMATCH-LONG"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
    );
    let pos_short = create_test_position(
        &instrument,
        PositionId::from("P-NET-MISMATCH-SHORT"),
        OrderSide::Sell,
        "2.0",
        "3100.00",
    );
    let report_long = create_test_position_report(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        "4.0",
        "P-NET-MISMATCH-LONG",
        dec!(3000.00),
    );
    let report_short = create_test_position_report(
        test_account_id(),
        instrument_id,
        PositionSide::Short,
        "1.0",
        "P-NET-MISMATCH-SHORT",
        dec!(3100.00),
    );
    ctx.add_instrument(instrument);
    ctx.add_position(&pos_long);
    ctx.add_position(&pos_short);

    let mock_client = MockPositionExecutionClient::new(vec![], vec![report_long, report_short]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(events.is_empty());
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, test_account_id())),
        1,
    );
}

#[tokio::test]
async fn test_position_check_venue_only_offset_hedge_legs_are_discrepant() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let report_long = create_test_position_report(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        "2.0",
        "P-OFFSET-LONG",
        dec!(3000.00),
    );
    let report_short = create_test_position_report(
        test_account_id(),
        instrument_id,
        PositionSide::Short,
        "2.0",
        "P-OFFSET-SHORT",
        dec!(3100.00),
    );
    ctx.add_instrument(instrument);

    let mock_client = MockPositionExecutionClient::new(vec![], vec![report_long, report_short]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(events.is_empty());
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, test_account_id())),
        1,
    );
}

#[tokio::test]
async fn test_position_check_flat_and_nonflat_reports_use_nonflat_report() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let position = create_test_position(
        &instrument,
        PositionId::from("P-NONFLAT"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
    );
    let nonflat_report = create_test_position_report(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        "5.0",
        "P-NONFLAT",
        dec!(3000.00),
    );

    let flat_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Flat,
        Quantity::from("0"),
        UnixNanos::from(2_000_000),
        UnixNanos::from(2_000_000),
        None,
        None,
        None,
    );

    ctx.add_instrument(instrument);
    ctx.add_position(&position);

    let mock_client = MockPositionExecutionClient::new(vec![], vec![nonflat_report, flat_report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(events.is_empty());
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, test_account_id())),
        0,
    );
}

#[tokio::test]
async fn test_position_check_multi_leg_discrepancy_defers_reconciliation() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let pos_long = create_test_position(
        &instrument,
        PositionId::from("P-DEFER-LONG"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
    );
    let pos_short = create_test_position(
        &instrument,
        PositionId::from("P-DEFER-SHORT"),
        OrderSide::Sell,
        "2.0",
        "3100.00",
    );
    let report_long = create_test_position_report(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        "5.0",
        "P-DEFER-LONG",
        dec!(3000.00),
    );
    let report_short = create_test_position_report(
        test_account_id(),
        instrument_id,
        PositionSide::Short,
        "1.0",
        "P-DEFER-SHORT",
        dec!(3100.00),
    );
    ctx.add_instrument(instrument);
    ctx.add_position(&pos_long);
    ctx.add_position(&pos_short);

    let mock_client = MockPositionExecutionClient::new(vec![], vec![report_long, report_short]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(events.is_empty());
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, test_account_id())),
        1,
    );
}

#[tokio::test]
async fn test_position_check_single_leg_gets_fresh_budget_after_multi_leg_exhaustion() {
    let config = ExecutionManagerConfig {
        position_check_retries: 1,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let pos_long = create_test_position(
        &instrument,
        PositionId::from("P-SHAPE-LONG"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
    );
    let pos_short = create_test_position(
        &instrument,
        PositionId::from("P-SHAPE-SHORT"),
        OrderSide::Sell,
        "2.0",
        "3100.00",
    );
    ctx.add_instrument(instrument);
    ctx.add_position(&pos_long);
    ctx.add_position(&pos_short);

    let report_long = create_test_position_report(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        "4.0",
        "P-SHAPE-LONG",
        dec!(3000.00),
    );
    let report_short = create_test_position_report(
        test_account_id(),
        instrument_id,
        PositionSide::Short,
        "2.0",
        "P-SHAPE-SHORT",
        dec!(3100.00),
    );
    let multi_leg_client =
        MockPositionExecutionClient::new(vec![], vec![report_long, report_short]);
    let clients: Vec<&dyn ExecutionClient> = vec![&multi_leg_client];

    let first_events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(first_events.is_empty());
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, test_account_id())),
        1,
    );

    let single_report = create_test_position_report(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        "4.0",
        "P-SHAPE-NET",
        dec!(3200.00),
    );
    let single_leg_client = MockPositionExecutionClient::new(vec![], vec![single_report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&single_leg_client];

    let second_events = ctx.manager.check_positions_consistency(&clients).await;

    let fills = second_events
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => Some(fill),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(fills.len(), 1);
    assert_eq!(fills[0].account_id, test_account_id());
    assert_eq!(fills[0].order_side, OrderSide::Buy);
    assert_eq!(fills[0].last_qty, Quantity::from("1.0"));
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, test_account_id())),
        0,
    );
}

#[tokio::test]
async fn test_position_check_multi_leg_reports_remain_isolated_by_account() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let account_a = AccountId::from("BINANCE-HEDGE-A");
    let account_b = AccountId::from("BINANCE-HEDGE-B");
    ctx.add_instrument(instrument.clone());
    ctx.add_margin_account(account_a);
    ctx.add_margin_account(account_b);

    for (position_id, side, qty, price, account_id) in [
        ("P-A-LONG", OrderSide::Buy, "5.0", "3000.00", account_a),
        ("P-A-SHORT", OrderSide::Sell, "2.0", "3100.00", account_a),
        ("P-B-LONG", OrderSide::Buy, "7.0", "3200.00", account_b),
        ("P-B-SHORT", OrderSide::Sell, "3.0", "3300.00", account_b),
    ] {
        let position = create_test_position_for_account(
            &instrument,
            PositionId::from(position_id),
            side,
            qty,
            price,
            account_id,
        );
        ctx.add_position(&position);
    }

    let client_a = MockPositionExecutionClient::configured(
        ClientId::from("BINANCE-HEDGE-A"),
        account_a,
        test_venue(),
        IndexSet::from([instrument_id.venue]),
        false,
    )
    .with_position_reports(vec![
        create_test_position_report(
            account_a,
            instrument_id,
            PositionSide::Long,
            "5.0",
            "P-A-LONG",
            dec!(3000.00),
        ),
        create_test_position_report(
            account_a,
            instrument_id,
            PositionSide::Short,
            "2.0",
            "P-A-SHORT",
            dec!(3100.00),
        ),
    ]);
    let client_b = MockPositionExecutionClient::configured(
        ClientId::from("BINANCE-HEDGE-B"),
        account_b,
        test_venue(),
        IndexSet::from([instrument_id.venue]),
        false,
    )
    .with_position_reports(vec![
        create_test_position_report(
            account_b,
            instrument_id,
            PositionSide::Long,
            "7.0",
            "P-B-LONG",
            dec!(3200.00),
        ),
        create_test_position_report(
            account_b,
            instrument_id,
            PositionSide::Short,
            "2.0",
            "P-B-SHORT",
            dec!(3300.00),
        ),
    ]);
    let clients: Vec<&dyn ExecutionClient> = vec![&client_a, &client_b];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(events.is_empty());
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, account_a)),
        0,
    );
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, account_b)),
        1,
    );
}

#[tokio::test]
async fn test_position_check_single_report_reconciliation_is_unchanged() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let position = create_test_position(
        &instrument,
        PositionId::from("P-SINGLE"),
        OrderSide::Buy,
        "3.0",
        "3000.00",
    );
    let report = create_test_position_report(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        "5.0",
        "P-SINGLE",
        dec!(3100.00),
    );
    ctx.add_instrument(instrument);
    ctx.add_position(&position);

    let mock_client = MockPositionExecutionClient::new(vec![], vec![report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    let fills = events
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => Some(fill),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(fills.len(), 1);
    assert_eq!(fills[0].account_id, test_account_id());
    assert_eq!(fills[0].order_side, OrderSide::Buy);
    assert_eq!(fills[0].last_qty, Quantity::from("2.0"));
    assert_eq!(fills[0].last_px, Price::from("3250.00"));
}

#[tokio::test]
async fn test_position_check_dedup_skips_second_hedge_position_same_instrument() {
    // Two hedge positions should only consume one retry per cycle, not two
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();

    let pos_long = create_test_position(
        &instrument,
        PositionId::from("P-LONG"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
    );
    let pos_short = create_test_position(
        &instrument,
        PositionId::from("P-SHORT"),
        OrderSide::Sell,
        "3.0",
        "3100.00",
    );

    // Omit instrument from cache to force the retry path
    ctx.add_position(&pos_long);
    ctx.add_position(&pos_short);

    let mock_client = MockExecutionClient::new(vec![]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    // Three cycles: each increments retry by 1 (not 2) thanks to dedup
    ctx.manager.check_positions_consistency(&clients).await;
    ctx.manager.check_positions_consistency(&clients).await;
    let events = ctx.manager.check_positions_consistency(&clients).await;
    assert!(events.is_empty());

    // With correct dedup retry count is 3 (= max), so adding the instrument
    // now should still be suppressed. If dedup were broken (2 per cycle),
    // retries would have hit 6 instead.
    ctx.add_instrument(instrument.clone());
    let events = ctx.manager.check_positions_consistency(&clients).await;
    assert!(
        events.is_empty(),
        "Expected retries to be exhausted after 3 cycles with dedup"
    );
}

#[tokio::test]
async fn test_position_check_flat_venue_report_does_not_protect_stale_counter() {
    // Flat (zero-qty) venue reports should not prevent pruning of stale
    // retry counters
    let config = ExecutionManagerConfig {
        position_check_retries: 1,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();

    ctx.add_instrument(instrument.clone());
    let position = create_test_position(
        &instrument,
        PositionId::from("P-001"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
    );
    ctx.add_position(&position);

    let flat_report = PositionStatusReport::new(
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

    let mock_client = MockPositionExecutionClient::new(vec![], vec![flat_report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;
    assert!(!events.is_empty());

    // Don't apply events - simulate recon not fixing it, exhaust retries
    ctx.manager.check_positions_consistency(&clients).await;
    let close_order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_id)
        .side(OrderSide::Sell)
        .quantity(Quantity::from("5.0"))
        .build();
    let close_fill = TestOrderEventStubs::filled(
        &close_order,
        &instrument,
        Some(TradeId::new("T-CLOSE-001")),
        Some(PositionId::from("P-001")),
        Some(Price::from("3000.00")),
        Some(Quantity::from("5.0")),
        None,
        None,
        None,
        Some(test_account_id()),
    );
    let close_filled: OrderFilled = close_fill.into();
    let mut pos = position;
    pos.apply(&close_filled);
    ctx.cache.borrow_mut().update_position(&pos).unwrap();

    // Flat venue report should not protect the stale retry counter
    ctx.manager.check_positions_consistency(&clients).await;

    // Counter was pruned, so new position should trigger recon
    let position2 = create_test_position(
        &instrument,
        PositionId::from("P-002"),
        OrderSide::Buy,
        "3.0",
        "3100.00",
    );
    ctx.add_position(&position2);

    let events = ctx.manager.check_positions_consistency(&clients).await;
    assert!(
        !events.is_empty(),
        "Expected events: flat venue report should not protect stale retry counter"
    );
}

#[tokio::test]
async fn test_position_check_nonflat_venue_report_protects_counter() {
    // Non-flat venue report should protect the retry counter from pruning
    let config = ExecutionManagerConfig {
        position_check_retries: 1,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();

    ctx.add_instrument(instrument.clone());
    let position = create_test_position(
        &instrument,
        PositionId::from("P-001"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
    );
    ctx.add_position(&position);

    let venue_report = PositionStatusReport::new(
        test_account_id(),
        instrument_id,
        PositionSide::Long,
        Quantity::from("3.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3000.00)),
    );

    let mock_client = MockPositionExecutionClient::new(vec![], vec![venue_report.clone()]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;
    assert!(!events.is_empty());

    // Don't apply events - exhaust retries
    ctx.manager.check_positions_consistency(&clients).await;
    let close_order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_id)
        .side(OrderSide::Sell)
        .quantity(Quantity::from("5.0"))
        .build();
    let close_fill = TestOrderEventStubs::filled(
        &close_order,
        &instrument,
        Some(TradeId::new("T-CLOSE-002")),
        Some(PositionId::from("P-001")),
        Some(Price::from("3000.00")),
        Some(Quantity::from("5.0")),
        None,
        None,
        None,
        Some(test_account_id()),
    );
    let close_filled: OrderFilled = close_fill.into();
    let mut pos = position;
    pos.apply(&close_filled);
    ctx.cache.borrow_mut().update_position(&pos).unwrap();

    // Non-flat venue report should keep the counter alive
    ctx.manager.check_positions_consistency(&clients).await;
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, test_account_id())),
        1,
    );

    let position2 = create_test_position(
        &instrument,
        PositionId::from("P-002"),
        OrderSide::Buy,
        "3.0",
        "3100.00",
    );
    ctx.add_position(&position2);

    // Counter retained - retries still exhausted
    let events = ctx.manager.check_positions_consistency(&clients).await;
    assert!(
        events.is_empty(),
        "Expected no events: non-flat venue report should protect retry counter"
    );
}

#[tokio::test]
async fn test_position_check_retries_independent_per_account() {
    // Two accounts on the same instrument must track their reconciliation
    // retry counters independently. With instrument-only keying, account A's
    // increment would suppress account B's first attempt entirely.
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();

    let account_a = AccountId::from("BINANCE-A");
    let account_b = AccountId::from("BINANCE-B");

    ctx.add_margin_account(account_a);
    ctx.add_margin_account(account_b);
    // Instrument deliberately omitted from cache: forces the failed-retry
    // path so each iteration that reaches it bumps the per-key counter.

    let pos_a = create_test_position_for_account(
        &instrument,
        PositionId::from("P-RETRY-A"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
        account_a,
    );
    let pos_b = create_test_position_for_account(
        &instrument,
        PositionId::from("P-RETRY-B"),
        OrderSide::Buy,
        "3.0",
        "3100.00",
        account_b,
    );
    ctx.add_position(&pos_a);
    ctx.add_position(&pos_b);

    let mock_client = MockExecutionClient::new(vec![]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    ctx.manager.check_positions_consistency(&clients).await;

    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, account_a)),
        1,
        "account A retry not incremented",
    );
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, account_b)),
        1,
        "account B retry not incremented (would be 0 if dedup collapsed by instrument)",
    );
}

#[tokio::test]
async fn test_position_check_activity_throttle_independent_per_account() {
    // Recent activity recorded for one account on an instrument must not
    // throttle reconciliation for another account on the same instrument.
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::from_mins(1), // 60s
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();

    let account_a = AccountId::from("BINANCE-A");
    let account_b = AccountId::from("BINANCE-B");

    ctx.add_margin_account(account_a);
    ctx.add_margin_account(account_b);
    ctx.add_instrument(instrument.clone());

    let pos_b = create_test_position_for_account(
        &instrument,
        PositionId::from("P-ACT-B"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
        account_b,
    );
    ctx.add_position(&pos_b);

    // Simulate recent activity on account A only: B must remain unthrottled.
    // Activity is stamped from the monotonic `dst::time` clock inside
    // `record_position_activity`, so no explicit timestamp is passed.
    ctx.manager
        .record_position_activity(instrument_id, account_a);

    // No venue report for B: treated as flat, so a discrepancy.
    let mock_client = MockPositionExecutionClient::new(vec![], vec![]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    let mut accounts_with_filled: HashSet<AccountId> = HashSet::new();

    for event in &events {
        if let OrderEventAny::Filled(fill) = event {
            accounts_with_filled.insert(fill.account_id);
        }
    }

    assert!(
        accounts_with_filled.contains(&account_b),
        "B's reconciliation must not be throttled by activity recorded for A",
    );
}

#[tokio::test]
async fn test_position_check_grace_survives_accelerated_trading_clock() {
    // The position-reconciliation grace is measured on the monotonic clock, not
    // `self.clock` (see `record_position_activity`). Here we record local activity,
    // then jump `self.clock` ~954 days forward in a single step, standing in for a
    // trading clock that has raced ahead while only milliseconds of real time have
    // elapsed, and assert the grace still suppresses the discrepancy.
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::from_mins(1), // 60s of real cover
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let account = AccountId::from("BINANCE-A");

    ctx.add_instrument(instrument.clone());
    ctx.add_margin_account(account);

    // Observe a fill: records local activity on the monotonic clock. The venue
    // event timestamps do not feed the grace and are left arbitrary.
    let ts_event = UnixNanos::from(1_000_000_000);

    let fill_report = FillReport::new(
        account,
        instrument_id,
        VenueOrderId::from("V-1"),
        TradeId::from("T-1"),
        OrderSide::Buy,
        Quantity::from("0.06"),
        Price::from("3000.00"),
        Money::new(0.0, Currency::USDT()),
        LiquiditySide::Taker,
        Some(ClientOrderId::from("O-1")),
        None,     // venue_position_id
        ts_event, // ts_event
        ts_event, // ts_init
        None,     // report_id
    );

    ctx.manager
        .observe_execution_report(&ExecutionReport::Fill(Box::new(fill_report)));

    // Race the trading clock ~954 days ahead. A `self.clock`-based grace would now
    // read "activity was 954 days ago" and fire; the monotonic grace must not.
    let accelerated_jump_ns: u64 = 954 * 86_400 * 1_000_000_000; // 954 days in ns
    ctx.advance_time(accelerated_jump_ns);

    // Cache is flat but the venue reports a 0.06 long: a genuine discrepancy. The
    // only thing between it and a synthesized EXTERNAL position is the grace.
    let venue_report = PositionStatusReport::new(
        account,
        instrument_id,
        PositionSide::Long,
        Quantity::from("0.06"),
        UnixNanos::from(accelerated_jump_ns),
        UnixNanos::from(accelerated_jump_ns),
        None, // report_id
        None, // venue_position_id
        Some(dec!(3000.00)),
    );

    let mock_client = MockPositionExecutionClient::new(vec![], vec![venue_report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(
        events.is_empty(),
        "grace must survive an accelerated trading clock (it is measured on the \
         monotonic clock); was {} reconciliation event(s), the grace regressed \
         to self.clock",
        events.len(),
    );
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, account)),
        0,
        "grace path must return before the retry counter is touched",
    );
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_position_check_grace_expires_on_monotonic_clock() {
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::from_mins(1),
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();
    let account = AccountId::from("BINANCE-A");

    ctx.add_instrument(instrument);
    ctx.add_margin_account(account);

    let ts_event = UnixNanos::from(1_000_000_000);

    let fill_report = FillReport::new(
        account,
        instrument_id,
        VenueOrderId::from("V-EXPIRY"),
        TradeId::from("T-EXPIRY"),
        OrderSide::Buy,
        Quantity::from("0.06"),
        Price::from("3000.00"),
        Money::new(0.0, Currency::USDT()),
        LiquiditySide::Taker,
        Some(ClientOrderId::from("O-EXPIRY")),
        None,
        ts_event,
        ts_event,
        None,
    );

    ctx.manager
        .observe_execution_report(&ExecutionReport::Fill(Box::new(fill_report)));

    advance_clock(dst::time::Duration::from_secs(61)).await;

    let venue_report = PositionStatusReport::new(
        account,
        instrument_id,
        PositionSide::Long,
        Quantity::from("0.06"),
        ts_event,
        ts_event,
        None,
        None,
        Some(dec!(3000.00)),
    );

    let mock_client = MockPositionExecutionClient::new(vec![], vec![venue_report]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    assert!(
        events
            .iter()
            .any(|event| matches!(event, OrderEventAny::Filled(_))),
        "position discrepancy should fire after the monotonic grace expires",
    );
}

#[tokio::test]
async fn test_check_positions_consistency_processes_only_discrepant_account() {
    // With cache and venue agreeing on account A and disagreeing on account B,
    // only B should be reconciled. A must remain untouched.
    let config = ExecutionManagerConfig {
        position_check_retries: 3,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();

    let account_a = AccountId::from("BINANCE-A");
    let account_b = AccountId::from("BINANCE-B");

    ctx.add_margin_account(account_a);
    ctx.add_margin_account(account_b);
    ctx.add_instrument(instrument.clone());

    let pos_a = create_test_position_for_account(
        &instrument,
        PositionId::from("P-E2E-A"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
        account_a,
    );
    let pos_b = create_test_position_for_account(
        &instrument,
        PositionId::from("P-E2E-B"),
        OrderSide::Buy,
        "3.0",
        "3100.00",
        account_b,
    );
    ctx.add_position(&pos_a);
    ctx.add_position(&pos_b);

    // Venue agrees with A; B has no report, so B is discrepant against flat.
    let report_a = PositionStatusReport::new(
        account_a,
        instrument_id,
        PositionSide::Long,
        Quantity::from("5.0"),
        UnixNanos::from(1_000_000),
        UnixNanos::from(1_000_000),
        None,
        None,
        Some(dec!(3000.00)),
    );

    let mock_client = MockPositionExecutionClient::new(vec![], vec![report_a]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    let events = ctx.manager.check_positions_consistency(&clients).await;

    let mut accounts_with_filled: HashSet<AccountId> = HashSet::new();

    for event in &events {
        if let OrderEventAny::Filled(fill) = event {
            accounts_with_filled.insert(fill.account_id);
        }
    }

    assert!(
        accounts_with_filled.contains(&account_b),
        "expected reconciliation events for the discrepant account B",
    );
    assert!(
        !accounts_with_filled.contains(&account_a),
        "account A is not discrepant and must not be reconciled",
    );
}

#[tokio::test]
async fn test_position_check_stale_retries_pruned_per_account() {
    // Mixed staleness across accounts on the same instrument: only the closed
    // account's retry counter should be pruned. The active account's counter
    // must be retained.
    let config = ExecutionManagerConfig {
        position_check_retries: 5,
        position_check_threshold_ns: DurationNanos::ZERO,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    let instrument_id = instrument.id();

    let account_a = AccountId::from("BINANCE-A");
    let account_b = AccountId::from("BINANCE-B");

    ctx.add_margin_account(account_a);
    ctx.add_margin_account(account_b);
    // Instrument deliberately omitted from cache: forces the failed-retry path.

    let pos_a = create_test_position_for_account(
        &instrument,
        PositionId::from("P-STALE-A"),
        OrderSide::Buy,
        "5.0",
        "3000.00",
        account_a,
    );
    let pos_b = create_test_position_for_account(
        &instrument,
        PositionId::from("P-STALE-B"),
        OrderSide::Buy,
        "3.0",
        "3100.00",
        account_b,
    );
    ctx.add_position(&pos_a);
    ctx.add_position(&pos_b);

    let mock_client = MockExecutionClient::new(vec![]);
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    // Cycle 1: both keys reach the failed-retry path, so counters land at 1 each.
    ctx.manager.check_positions_consistency(&clients).await;
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, account_a)),
        1,
    );
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, account_b)),
        1,
    );

    // Close account B's position so it disappears from open_positions.
    let close_order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_id)
        .side(OrderSide::Sell)
        .quantity(Quantity::from("3.0"))
        .build();
    let close_fill = TestOrderEventStubs::filled(
        &close_order,
        &instrument,
        Some(TradeId::new("T-CLOSE-B")),
        Some(PositionId::from("P-STALE-B")),
        Some(Price::from("3100.00")),
        Some(Quantity::from("3.0")),
        None,
        None,
        None,
        Some(account_b),
    );
    let close_filled: OrderFilled = close_fill.into();
    let mut pos_b = pos_b;
    pos_b.apply(&close_filled);
    ctx.cache.borrow_mut().update_position(&pos_b).unwrap();

    // Cycle 2: A still active and discrepant, so its counter increments;
    // B's position is closed and venue reports nothing, so its counter is pruned.
    ctx.manager.check_positions_consistency(&clients).await;

    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, account_a)),
        2,
        "account A's counter must be retained while it remains active",
    );
    assert_eq!(
        ctx.manager
            .position_recon_retry_count(&(instrument_id, account_b)),
        0,
        "account B's counter must be pruned once its position is closed",
    );
}

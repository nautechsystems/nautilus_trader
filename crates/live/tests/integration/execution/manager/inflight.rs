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

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_observe_order_report_clears_inflight_tracking() {
    let config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 5,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");

    ctx.add_instrument(test_instrument());
    let order = create_submitted_order("O-001", instrument_id, OrderSide::Buy, "1.0", "3000.00");
    ctx.add_order(order);

    ctx.manager.register_inflight(client_order_id, UUID4::new());

    // Simulate venue responding with an OrderStatusReport
    let order_report = create_order_report(
        Some(client_order_id),
        VenueOrderId::from("V-001"),
        instrument_id,
        OrderStatus::Accepted,
        Quantity::from("1.0"),
        Quantity::from(0),
    );
    let report = ExecutionReport::Order(Box::new(order_report));

    ctx.manager.observe_execution_report(&report);

    // Advance time past threshold so inflight check would trigger if still tracked
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;

    // Inflight check should return empty - tracking was cleared by observation
    let result = ctx.manager.check_inflight_orders();
    assert!(result.events.is_empty());
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_observe_pending_order_report_keeps_inflight_tracking() {
    let config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 3,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-PENDING");

    ctx.add_instrument(test_instrument());
    let order =
        create_submitted_order("O-PENDING", instrument_id, OrderSide::Buy, "1.0", "3000.00");
    ctx.add_order(order);

    ctx.manager.register_inflight(client_order_id, UUID4::new());

    // Venue reports PendingUpdate (interim acknowledgement, not terminal)
    let order_report = create_order_report(
        Some(client_order_id),
        VenueOrderId::from("V-001"),
        instrument_id,
        OrderStatus::PendingUpdate,
        Quantity::from("1.0"),
        Quantity::from(0),
    );
    let report = ExecutionReport::Order(Box::new(order_report));

    ctx.manager.observe_execution_report(&report);

    // Inflight tracking should still be active
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;
    let result = ctx.manager.check_inflight_orders();

    // Order is still tracked: should generate a query (retry 1 < max 3)
    assert_eq!(result.queries.len(), 1);
    assert!(result.events.is_empty());
}

#[rstest]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_inflight_order_generates_rejection_after_max_retries(
    #[values(false, true)] submission: bool,
) {
    let config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 1,
        submission_recovery_policy: SubmissionRecoveryPolicy::ResolveLocally,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");

    ctx.add_instrument(test_instrument());

    // Order must be submitted (have account_id) to generate rejection
    let order = create_submitted_order("O-001", instrument_id, OrderSide::Buy, "1.0", "3000.00");
    ctx.add_order(order.clone());
    if submission {
        ctx.manager
            .register_submission(order.init_event(), Some(test_client_id()));
    } else {
        ctx.manager.register_inflight(client_order_id, UUID4::new());
    }

    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await; // 200ms, past threshold

    let result = ctx.manager.check_inflight_orders();

    assert_eq!(result.events.len(), 1);
    assert!(matches!(result.events[0], OrderEventAny::Rejected(_)));

    if let OrderEventAny::Rejected(rejected) = &result.events[0] {
        assert_eq!(rejected.client_order_id, client_order_id);
        assert_eq!(rejected.reason, "INFLIGHT_TIMEOUT");
    }

    let diagnostics = ctx.manager.take_submission_recovery_exhaustions();
    let order = ctx.get_order(&client_order_id).unwrap();
    assert_eq!(diagnostics, Vec::new());
    assert!(
        ctx.manager
            .take_submission_recovery_exhaustions()
            .is_empty()
    );
    assert_eq!(order.status(), OrderStatus::Submitted);
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_inflight_timeout_uses_monotonic_gate_and_domain_event_timestamp() {
    let config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 1,
        submission_recovery_policy: SubmissionRecoveryPolicy::ResolveLocally,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-SPLIT");

    ctx.add_instrument(test_instrument());
    let order = create_submitted_order("O-SPLIT", instrument_id, OrderSide::Buy, "1.0", "3000.00");
    ctx.add_order(order);

    ctx.manager.register_inflight(client_order_id, UUID4::new());
    let domain_ts = ctx.clock.borrow().timestamp_ns();
    advance_clock(dst::time::Duration::from_millis(200)).await;

    let result = ctx.manager.check_inflight_orders();

    assert_eq!(result.events.len(), 1);
    assert_eq!(result.events[0].ts_event(), domain_ts);
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_inflight_check_skips_filtered_order_ids() {
    let filtered_id = ClientOrderId::from("O-FILTERED");

    let mut config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 1,
        ..Default::default()
    };

    config.filtered_client_order_ids.insert(filtered_id);
    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();

    ctx.add_instrument(test_instrument());

    // Use submitted order (has account_id) to verify filtering, not missing account_id
    let order = create_submitted_order(
        "O-FILTERED",
        instrument_id,
        OrderSide::Buy,
        "1.0",
        "3000.00",
    );
    ctx.add_order(order);

    ctx.manager.register_inflight(filtered_id, UUID4::new());
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;

    let result = ctx.manager.check_inflight_orders();

    // Filtered order should not generate rejection
    assert!(result.events.is_empty());
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_inflight_increments_retry_count_before_max() {
    let config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 3,
        submission_recovery_policy: SubmissionRecoveryPolicy::ResolveLocally,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-001");

    ctx.add_instrument(test_instrument());
    let order = create_submitted_order("O-001", instrument_id, OrderSide::Buy, "1.0", "3000.00");
    ctx.add_order(order);

    ctx.manager.register_inflight(client_order_id, UUID4::new());

    // First check - past threshold, retry count becomes 1, generates QueryOrder
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;
    let result1 = ctx.manager.check_inflight_orders();
    assert!(result1.events.is_empty()); // Not at max yet
    assert_eq!(result1.queries.len(), 1);

    // Second check - retry count becomes 2, generates QueryOrder
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;
    let result2 = ctx.manager.check_inflight_orders();
    assert!(result2.events.is_empty()); // Still not at max
    assert_eq!(result2.queries.len(), 1);

    // Third check - retry count becomes 3, equals max, generates rejection
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;
    let result3 = ctx.manager.check_inflight_orders();
    assert_eq!(result3.events.len(), 1);
    assert!(matches!(result3.events[0], OrderEventAny::Rejected(_)));
    assert!(result3.queries.is_empty());
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_inflight_pending_update_generates_canceled() {
    let config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 1,
        submission_recovery_policy: SubmissionRecoveryPolicy::ResolveLocally,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-PENDING-UPD");
    let venue_order_id = VenueOrderId::from("V-001");

    ctx.add_instrument(test_instrument());

    let order = create_pending_update_order(
        "O-PENDING-UPD",
        instrument_id,
        OrderSide::Buy,
        "1.0",
        "3000.00",
        venue_order_id,
    );
    ctx.add_order(order);

    ctx.manager.register_inflight(client_order_id, UUID4::new());
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await; // 200ms, past threshold

    let result = ctx.manager.check_inflight_orders();

    assert!(
        ctx.manager
            .take_submission_recovery_exhaustions()
            .is_empty()
    );
    assert_eq!(result.events.len(), 1);
    assert!(
        matches!(result.events[0], OrderEventAny::Canceled(_)),
        "Expected Canceled for PendingUpdate, was {:?}",
        result.events[0]
    );

    if let OrderEventAny::Canceled(canceled) = &result.events[0] {
        assert_eq!(canceled.client_order_id, client_order_id);
        assert_eq!(canceled.reason.as_deref(), Some("INFLIGHT_TIMEOUT"));
    }
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_inflight_pending_cancel_generates_canceled() {
    let config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 1,
        submission_recovery_policy: SubmissionRecoveryPolicy::ResolveLocally,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-PENDING-CXL");
    let venue_order_id = VenueOrderId::from("V-002");

    ctx.add_instrument(test_instrument());

    let order = create_pending_cancel_order(
        "O-PENDING-CXL",
        instrument_id,
        OrderSide::Buy,
        "1.0",
        "3000.00",
        venue_order_id,
    );
    ctx.add_order(order);

    ctx.manager.register_inflight(client_order_id, UUID4::new());
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;

    let result = ctx.manager.check_inflight_orders();

    assert!(
        ctx.manager
            .take_submission_recovery_exhaustions()
            .is_empty()
    );
    assert_eq!(result.events.len(), 1);
    assert!(
        matches!(result.events[0], OrderEventAny::Canceled(_)),
        "Expected Canceled for PendingCancel, was {:?}",
        result.events[0]
    );

    if let OrderEventAny::Canceled(canceled) = &result.events[0] {
        assert_eq!(canceled.client_order_id, client_order_id);
        assert_eq!(canceled.reason.as_deref(), Some("INFLIGHT_TIMEOUT"));
    }
}

#[rstest]
#[case::pending_update(true, false)]
#[case::pending_cancel(false, true)]
#[case::pending_cancel_after_update(true, true)]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_inflight_pending_command_restores_status_under_retention(
    #[case] pending_update: bool,
    #[case] pending_cancel: bool,
) {
    let config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 1,
        submission_recovery_policy: SubmissionRecoveryPolicy::RetainUnresolved,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-PENDING");
    let venue_order_id = VenueOrderId::from("V-003");
    ctx.add_instrument(test_instrument());

    let mut order = if pending_update {
        create_pending_update_order(
            "O-PENDING",
            instrument_id,
            OrderSide::Buy,
            "1.0",
            "3000.00",
            venue_order_id,
        )
    } else {
        create_pending_cancel_order(
            "O-PENDING",
            instrument_id,
            OrderSide::Buy,
            "1.0",
            "3000.00",
            venue_order_id,
        )
    };

    if pending_update && pending_cancel {
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
        order.apply(pending).unwrap();
    }

    ctx.add_order(order);
    ctx.manager.register_inflight(client_order_id, UUID4::new());
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;

    let result = ctx.manager.check_inflight_orders();
    for event in &result.events {
        ctx.manager.observe_order_event(event);
        ctx.exec_engine.borrow_mut().process(event);
    }

    let mut expected = Vec::new();
    if pending_cancel {
        expected.push("CancelRejected");
    }

    if pending_update {
        expected.push("ModifyRejected");
    }

    let rejections: Vec<_> = result
        .events
        .iter()
        .map(|event| match event {
            OrderEventAny::CancelRejected(event) => (
                "CancelRejected",
                event.client_order_id,
                event.reason.as_str(),
                event.reconciliation,
                event.venue_order_id,
                event.account_id,
            ),
            OrderEventAny::ModifyRejected(event) => (
                "ModifyRejected",
                event.client_order_id,
                event.reason.as_str(),
                event.reconciliation,
                event.venue_order_id,
                event.account_id,
            ),
            other => panic!("Expected a command rejection, was {other:?}"),
        })
        .collect();

    let expected: Vec<_> = expected
        .into_iter()
        .map(|kind| {
            (
                kind,
                client_order_id,
                "INFLIGHT_TIMEOUT",
                true,
                Some(venue_order_id),
                Some(test_account_id()),
            )
        })
        .collect();

    let order = ctx.get_order(&client_order_id).unwrap();
    assert_eq!(rejections, expected);
    assert!(
        ctx.manager
            .take_submission_recovery_exhaustions()
            .is_empty()
    );
    assert_eq!(order.status(), OrderStatus::Accepted);
    assert_eq!(order.ts_closed(), None);
    assert!(!order.has_pending_update_command());
    assert!(!order.has_pending_cancel_command());
    assert!(
        ctx.cache
            .borrow()
            .orders_open(None, None, None, None, None)
            .into_iter()
            .any(|order| order.client_order_id() == client_order_id)
    );
    assert!(ctx.manager.check_inflight_orders().events.is_empty());
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_inflight_generates_query_before_max_retries() {
    let config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 3,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-QUERY");

    ctx.add_instrument(test_instrument());
    let order = create_submitted_order("O-QUERY", instrument_id, OrderSide::Buy, "1.0", "3000.00");
    ctx.add_order(order);

    ctx.manager.register_inflight(client_order_id, UUID4::new());

    // First check - past threshold, retry 1 < max 3 -> generates QueryOrder
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;
    let result = ctx.manager.check_inflight_orders();

    assert!(
        result.events.is_empty(),
        "Should not generate terminal events yet"
    );
    assert_eq!(result.queries.len(), 1, "Should generate one QueryOrder");

    if let TradingCommand::QueryOrder(query) = &result.queries[0] {
        assert_eq!(query.client_order_id, client_order_id);
    } else {
        panic!("Expected QueryOrder, was {:?}", result.queries[0]);
    }
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_inflight_no_query_at_max_retries() {
    let config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 2,
        submission_recovery_policy: SubmissionRecoveryPolicy::ResolveLocally,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-MAX");

    ctx.add_instrument(test_instrument());
    let order = create_submitted_order("O-MAX", instrument_id, OrderSide::Buy, "1.0", "3000.00");
    ctx.add_order(order);

    ctx.manager.register_inflight(client_order_id, UUID4::new());

    // First check - intermediate, generates query
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;
    let result1 = ctx.manager.check_inflight_orders();
    assert!(result1.events.is_empty());
    assert_eq!(result1.queries.len(), 1);

    // Second check - at max retries, generates terminal event only
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;
    let result2 = ctx.manager.check_inflight_orders();

    assert_eq!(
        result2.events.len(),
        1,
        "Should generate terminal event at max retries"
    );
    assert!(
        result2.queries.is_empty(),
        "Should not generate queries at max retries"
    );
    assert!(matches!(result2.events[0], OrderEventAny::Rejected(_)));
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_inflight_query_preserves_client_id_routing() {
    let config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 3,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-ROUTED");
    let client_id = ClientId::from("EXEC-002");

    ctx.add_instrument(test_instrument());
    let order = create_submitted_order("O-ROUTED", instrument_id, OrderSide::Buy, "1.0", "3000.00");
    ctx.add_order_with_client_id(order, client_id);

    ctx.manager.register_inflight(client_order_id, UUID4::new());
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;

    let result = ctx.manager.check_inflight_orders();
    assert_eq!(result.queries.len(), 1);

    if let TradingCommand::QueryOrder(query) = &result.queries[0] {
        assert_eq!(
            query.client_id,
            Some(client_id),
            "QueryOrder should preserve the execution client routing"
        );
        assert_eq!(query.client_order_id, client_order_id);
    } else {
        panic!("Expected QueryOrder, was {:?}", result.queries[0]);
    }
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_inflight_query_throttled_within_threshold() {
    let config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 5,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-THROTTLE");

    ctx.add_instrument(test_instrument());
    let order = create_submitted_order(
        "O-THROTTLE",
        instrument_id,
        OrderSide::Buy,
        "1.0",
        "3000.00",
    );
    ctx.add_order(order);

    ctx.manager.register_inflight(client_order_id, UUID4::new());

    // First check past threshold generates a query
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;
    let result1 = ctx.manager.check_inflight_orders();
    assert_eq!(result1.queries.len(), 1);

    // Immediate second check without advancing time is throttled
    let result2 = ctx.manager.check_inflight_orders();
    assert!(result2.queries.is_empty(), "Query should be throttled");
    assert!(result2.events.is_empty());

    // Advance past threshold again, query should fire
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;
    let result3 = ctx.manager.check_inflight_orders();
    assert_eq!(result3.queries.len(), 1);
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_inflight_accepted_order_at_max_retries_no_event() {
    let config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 1,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-ACCEPTED");
    let venue_order_id = VenueOrderId::from("V-001");

    ctx.add_instrument(test_instrument());
    let order = create_accepted_order(
        "O-ACCEPTED",
        instrument_id,
        OrderSide::Buy,
        "1.0",
        "3000.00",
        venue_order_id,
    );
    ctx.add_order(order);

    ctx.manager.register_inflight(client_order_id, UUID4::new());
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;

    let result = ctx.manager.check_inflight_orders();

    // Accepted orders are already resolved, no terminal event generated
    assert!(result.events.is_empty());
    assert!(result.queries.is_empty());

    // Tracking should be cleared, subsequent check also empty
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;
    let result2 = ctx.manager.check_inflight_orders();
    assert!(result2.events.is_empty());
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_inflight_order_not_in_cache_at_max_retries_no_event() {
    let config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 1,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let client_order_id = ClientOrderId::from("O-MISSING");

    // Register inflight without adding order to cache
    ctx.manager.register_inflight(client_order_id, UUID4::new());
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;

    let result = ctx.manager.check_inflight_orders();

    assert!(result.events.is_empty());
    assert!(result.queries.is_empty());

    // Tracking cleared, subsequent check also empty
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;
    let result2 = ctx.manager.check_inflight_orders();
    assert!(result2.events.is_empty());
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_inflight_terminal_event_clears_tracking() {
    let config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 1,
        submission_recovery_policy: SubmissionRecoveryPolicy::ResolveLocally,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument_id = test_instrument_id();
    let client_order_id = ClientOrderId::from("O-TERM");

    ctx.add_instrument(test_instrument());
    let order = create_submitted_order("O-TERM", instrument_id, OrderSide::Buy, "1.0", "3000.00");
    ctx.add_order(order);

    ctx.manager.register_inflight(client_order_id, UUID4::new());
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;

    // First check generates terminal rejection
    let result1 = ctx.manager.check_inflight_orders();
    assert_eq!(result1.events.len(), 1);
    assert!(matches!(result1.events[0], OrderEventAny::Rejected(_)));

    // Second check should return empty (tracking was cleared)
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;
    let result2 = ctx.manager.check_inflight_orders();
    assert!(result2.events.is_empty());
    assert!(result2.queries.is_empty());
}

#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_clear_recon_tracking_removes_inflight() {
    let config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 5,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let client_order_id = ClientOrderId::from("O-001");

    ctx.manager.register_inflight(client_order_id, UUID4::new());

    // Simulate order being resolved externally (e.g., accepted by venue)
    ctx.manager.clear_recon_tracking(&client_order_id, true);

    // Advance time past threshold
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;

    // Check should not generate events since order was cleared
    let result = ctx.manager.check_inflight_orders();
    assert!(result.events.is_empty());
}

fn create_pending_cancel_order(
    client_order_id: &str,
    instrument_id: InstrumentId,
    side: OrderSide,
    quantity: &str,
    price: &str,
    venue_order_id: VenueOrderId,
) -> OrderAny {
    let mut order = create_accepted_order(
        client_order_id,
        instrument_id,
        side,
        quantity,
        price,
        venue_order_id,
    );
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
    order.apply(pending).unwrap();
    order
}

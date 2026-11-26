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
#[case(1)]
#[case(3)]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_submission_registry_preserves_identity_and_bounded_queries(#[case] budget: u32) {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: budget,
        submission_recovery_policy: SubmissionRecoveryPolicy::RetainUnresolved,
        ..Default::default()
    });

    ctx.add_instrument(test_instrument());
    let order = create_submitted_order(
        "O-REGISTRY",
        test_instrument_id(),
        OrderSide::Buy,
        "1.0",
        "3000.00",
    );
    let initialized = order.init_event().clone();
    let client_order_id = order.client_order_id();
    ctx.manager
        .register_submission(&initialized, Some(test_client_id()));
    ctx.add_order(order);

    let mut query_count = 0;

    for check in 1..=budget {
        ctx.advance_both(dst::time::Duration::from_millis(101))
            .await;
        let result = ctx.manager.check_inflight_orders();
        query_count += result.queries.len();

        if check < budget {
            assert!(result.events.is_empty());
            assert_eq!(result.queries.len(), 1);

            let TradingCommand::QueryOrder(query) = &result.queries[0] else {
                panic!("Recovery must only query an existing submission");
            };

            assert_eq!(query.trader_id, initialized.trader_id);
            assert_eq!(query.client_id, Some(test_client_id()));
            assert_eq!(query.strategy_id, initialized.strategy_id);
            assert_eq!(query.instrument_id, initialized.instrument_id);
            assert_eq!(query.client_order_id, client_order_id);
            assert_eq!(query.venue_order_id, None);
            assert_eq!(query.ts_init, ctx.clock.borrow().timestamp_ns());
            assert!(
                ctx.manager
                    .take_submission_recovery_exhaustions()
                    .is_empty()
            );

            let mut duplicate = initialized.clone();
            duplicate.trader_id = TraderId::from("OTHER-001");
            ctx.manager
                .register_submission(&duplicate, Some(ClientId::from("OTHER")));
        } else {
            assert!(result.queries.is_empty());
            assert!(result.events.is_empty());
            assert_eq!(
                ctx.get_order(&client_order_id).unwrap().status(),
                OrderStatus::Submitted
            );
            assert_eq!(
                ctx.manager.take_submission_recovery_exhaustions(),
                vec![SubmissionRecoveryExhausted {
                    trader_id: initialized.trader_id,
                    client_id: Some(test_client_id()),
                    strategy_id: initialized.strategy_id,
                    instrument_id: initialized.instrument_id,
                    client_order_id,
                    source: SubmissionRecoverySource::Inflight,
                    retry_count: budget,
                    ts_event: ctx.clock.borrow().timestamp_ns(),
                }],
            );
        }
    }

    ctx.advance_both(dst::time::Duration::from_millis(101))
        .await;
    let after_exhaustion = ctx.manager.check_inflight_orders();
    assert_eq!(query_count, usize::try_from(budget - 1).unwrap());
    assert!(after_exhaustion.events.is_empty());
    assert!(after_exhaustion.queries.is_empty());
    assert!(
        ctx.manager
            .take_submission_recovery_exhaustions()
            .is_empty()
    );
}

#[rstest]
#[case::unfiltered(false)]
#[case::filtered(true)]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_submission_registry_before_cache_insertion(#[case] filtered: bool) {
    let order = create_limit_order(
        "O-PRECACHE",
        test_instrument_id(),
        OrderSide::Buy,
        "1.0",
        "3000.00",
    );
    let client_order_id = order.client_order_id();
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 1,
        submission_recovery_policy: SubmissionRecoveryPolicy::RetainUnresolved,
        filtered_client_order_ids: if filtered {
            IndexSet::from([client_order_id])
        } else {
            IndexSet::new()
        },
        ..Default::default()
    });

    ctx.manager
        .register_submission(order.init_event(), Some(test_client_id()));
    ctx.advance_both(dst::time::Duration::from_millis(101))
        .await;

    let result = ctx.manager.check_inflight_orders();
    let diagnostics = ctx.manager.take_submission_recovery_exhaustions();

    assert!(result.queries.is_empty());
    assert!(result.events.is_empty());
    assert_eq!(diagnostics.len(), usize::from(!filtered));

    if !filtered {
        assert_eq!(
            diagnostics[0],
            SubmissionRecoveryExhausted {
                trader_id: order.trader_id(),
                client_id: Some(test_client_id()),
                strategy_id: order.strategy_id(),
                instrument_id: order.instrument_id(),
                client_order_id,
                source: SubmissionRecoverySource::Inflight,
                retry_count: 1,
                ts_event: ctx.clock.borrow().timestamp_ns(),
            },
        );
    }

    assert!(
        ctx.manager
            .take_submission_recovery_exhaustions()
            .is_empty()
    );
}

#[rstest]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_submission_registry_applied_acceptance_prevents_exhaustion() {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 1,
        submission_recovery_policy: SubmissionRecoveryPolicy::RetainUnresolved,
        ..Default::default()
    });

    ctx.add_instrument(test_instrument());
    let order = create_submitted_order(
        "O-DELAYED-ACCEPT",
        test_instrument_id(),
        OrderSide::Buy,
        "1.0",
        "3000.00",
    );
    let accepted = TestOrderEventStubs::accepted(
        &order,
        test_account_id(),
        VenueOrderId::from("V-DELAYED-ACCEPT"),
    );
    ctx.manager
        .register_submission(order.init_event(), Some(test_client_id()));
    ctx.add_order(order);
    ctx.cache.borrow_mut().update_order(&accepted).unwrap();
    ctx.advance_both(dst::time::Duration::from_millis(101))
        .await;

    let result = ctx.manager.check_inflight_orders();

    assert!(result.events.is_empty());
    assert!(result.queries.is_empty());
    assert!(ctx.manager.check_open_order_queries().is_empty());
    assert!(
        ctx.manager
            .take_submission_recovery_exhaustions()
            .is_empty()
    );
}

#[rstest]
#[case::cancel_rejected("cancel_rejected")]
#[case::modify_rejected("modify_rejected")]
#[case::updated("updated")]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_submission_registry_survives_unacknowledged_command_response(
    #[case] response: &str,
    #[values(
        SubmissionRecoveryPolicy::ResolveLocally,
        SubmissionRecoveryPolicy::RetainUnresolved
    )]
    policy: SubmissionRecoveryPolicy,
) {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 2,
        submission_recovery_policy: policy,
        ..Default::default()
    });

    let order = create_submitted_order(
        "O-REGISTRY-RESPONSE",
        test_instrument_id(),
        OrderSide::Buy,
        "1.0",
        "3000.00",
    );
    let initialized = order.init_event().clone();
    let client_order_id = order.client_order_id();
    ctx.manager
        .register_submission(&initialized, Some(test_client_id()));
    ctx.add_order(order.clone());
    ctx.advance_both(dst::time::Duration::from_millis(101))
        .await;
    let first = ctx.manager.check_inflight_orders();
    assert!(first.events.is_empty());
    assert_eq!(first.queries.len(), 1);

    let pending = if response == "cancel_rejected" {
        OrderEventAny::PendingCancel(
            OrderPendingCancelSpec::builder()
                .trader_id(order.trader_id())
                .strategy_id(order.strategy_id())
                .instrument_id(order.instrument_id())
                .client_order_id(client_order_id)
                .account_id(test_account_id())
                .build(),
        )
    } else {
        OrderEventAny::PendingUpdate(
            OrderPendingUpdateSpec::builder()
                .trader_id(order.trader_id())
                .strategy_id(order.strategy_id())
                .instrument_id(order.instrument_id())
                .client_order_id(client_order_id)
                .account_id(test_account_id())
                .build(),
        )
    };

    ctx.manager.observe_order_event(&pending);
    ctx.cache.borrow_mut().update_order(&pending).unwrap();
    ctx.manager.register_inflight(client_order_id, UUID4::new());

    let event = match response {
        "cancel_rejected" => OrderEventAny::CancelRejected(
            OrderCancelRejectedSpec::builder()
                .trader_id(order.trader_id())
                .strategy_id(order.strategy_id())
                .instrument_id(order.instrument_id())
                .client_order_id(client_order_id)
                .account_id(test_account_id())
                .build(),
        ),
        "modify_rejected" => OrderEventAny::ModifyRejected(
            OrderModifyRejectedSpec::builder()
                .trader_id(order.trader_id())
                .strategy_id(order.strategy_id())
                .instrument_id(order.instrument_id())
                .client_order_id(client_order_id)
                .account_id(test_account_id())
                .build(),
        ),
        _ => OrderEventAny::Updated(
            OrderUpdatedSpec::builder()
                .trader_id(order.trader_id())
                .strategy_id(order.strategy_id())
                .instrument_id(order.instrument_id())
                .client_order_id(client_order_id)
                .account_id(test_account_id())
                .quantity(order.quantity())
                .price(Price::from("3000.00"))
                .build(),
        ),
    };

    ctx.manager.observe_order_event(&event);
    ctx.cache.borrow_mut().update_order(&event).unwrap();
    assert_eq!(
        ctx.get_order(&client_order_id).unwrap().status(),
        OrderStatus::Submitted
    );

    let mut duplicate = initialized.clone();
    duplicate.trader_id = TraderId::from("OTHER-001");
    ctx.manager
        .register_submission(&duplicate, Some(ClientId::from("OTHER")));
    let tracking = policy == SubmissionRecoveryPolicy::RetainUnresolved;
    assert_eq!(
        ctx.manager.recon_check_retry_count(&client_order_id),
        u32::from(tracking)
    );
    ctx.advance_both(dst::time::Duration::from_millis(101))
        .await;
    let mut result = ctx.manager.check_inflight_orders();

    if !tracking {
        assert_eq!(result.queries.len(), 1);
        assert!(result.events.is_empty());
        ctx.advance_both(dst::time::Duration::from_millis(101))
            .await;
        result = ctx.manager.check_inflight_orders();
    }

    assert!(result.queries.is_empty());

    if tracking {
        assert!(result.events.is_empty());
        assert_eq!(
            ctx.get_order(&client_order_id).unwrap().status(),
            OrderStatus::Submitted
        );
    } else {
        assert!(
            matches!(&result.events[..], [OrderEventAny::Rejected(event)]
        if event.client_order_id == client_order_id && event.reason == "INFLIGHT_TIMEOUT")
        );
    }

    let expected = if tracking {
        vec![SubmissionRecoveryExhausted {
            trader_id: initialized.trader_id,
            client_id: Some(test_client_id()),
            strategy_id: initialized.strategy_id,
            instrument_id: initialized.instrument_id,
            client_order_id,
            source: SubmissionRecoverySource::Inflight,
            retry_count: 2,
            ts_event: ctx.clock.borrow().timestamp_ns(),
        }]
    } else {
        Vec::new()
    };

    assert_eq!(ctx.manager.take_submission_recovery_exhaustions(), expected);
    assert!(
        ctx.manager
            .take_submission_recovery_exhaustions()
            .is_empty()
    );
}

#[rstest]
#[case::unacknowledged_cancel(false, OrderStatus::PendingCancel)]
#[case::unacknowledged_modify(false, OrderStatus::PendingUpdate)]
#[case::acknowledged_cancel(true, OrderStatus::PendingCancel)]
#[case::acknowledged_modify(true, OrderStatus::PendingUpdate)]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_submission_registry_interleaved_missing_checks_preserve_budget(
    #[case] acknowledged: bool,
    #[case] pending_status: OrderStatus,
    #[values(
        SubmissionRecoveryPolicy::ResolveLocally,
        SubmissionRecoveryPolicy::RetainUnresolved
    )]
    policy: SubmissionRecoveryPolicy,
) {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        inflight_threshold_ms: 200,
        inflight_max_retries: 2,
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 1,
        open_check_open_only: false,
        single_order_query_delay_ms: 0,
        submission_recovery_policy: policy,
        ..Default::default()
    });

    ctx.add_instrument(test_instrument());
    let order = create_limit_order(
        "O-REGISTRY-INTERLEAVED",
        test_instrument_id(),
        OrderSide::Buy,
        "1.0",
        "3000.00",
    );
    let client_order_id = order.client_order_id();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    ctx.add_order(order.clone());
    ctx.cache.borrow_mut().update_order(&submitted).unwrap();

    if acknowledged {
        let accepted = TestOrderEventStubs::accepted(
            &order,
            test_account_id(),
            VenueOrderId::from("V-INTERLEAVED"),
        );
        ctx.cache.borrow_mut().update_order(&accepted).unwrap();
    }

    ctx.manager
        .register_submission(order.init_event(), Some(test_client_id()));

    let pending = if pending_status == OrderStatus::PendingCancel {
        OrderEventAny::PendingCancel(
            OrderPendingCancelSpec::builder()
                .trader_id(order.trader_id())
                .strategy_id(order.strategy_id())
                .instrument_id(order.instrument_id())
                .client_order_id(client_order_id)
                .account_id(test_account_id())
                .build(),
        )
    } else {
        OrderEventAny::PendingUpdate(
            OrderPendingUpdateSpec::builder()
                .trader_id(order.trader_id())
                .strategy_id(order.strategy_id())
                .instrument_id(order.instrument_id())
                .client_order_id(client_order_id)
                .account_id(test_account_id())
                .build(),
        )
    };

    ctx.cache.borrow_mut().update_order(&pending).unwrap();
    ctx.manager.register_inflight(client_order_id, UUID4::new());
    let client = MockExecutionClient::new(Vec::new());
    let tracked = policy == SubmissionRecoveryPolicy::RetainUnresolved && !acknowledged;
    let mut queries = 0;
    let mut resolutions = Vec::new();
    let mut diagnostics = Vec::new();

    for _ in 0..5 {
        ctx.advance_both(dst::time::Duration::from_millis(101))
            .await;
        assert!(ctx.manager.check_open_orders(&[&client]).await.is_empty());
        let result = ctx.manager.check_inflight_orders();
        queries += result.queries.len();
        for event in &result.events {
            ctx.cache.borrow_mut().update_order(event).unwrap();
        }

        resolutions.extend(result.events);
        diagnostics.extend(ctx.manager.take_submission_recovery_exhaustions());
    }

    assert_eq!(queries, usize::from(tracked));
    assert!(resolutions.is_empty());
    assert_eq!(
        client.order_report_query_count.get(),
        if tracked { 4 } else { 5 }
    );
    assert_eq!(
        ctx.get_order(&client_order_id).unwrap().status(),
        pending_status
    );
    assert_eq!(diagnostics.len(), usize::from(tracked));

    if tracked {
        assert_eq!(diagnostics[0].client_order_id, client_order_id);
        assert_eq!(diagnostics[0].source, SubmissionRecoverySource::Inflight);
        assert_eq!(diagnostics[0].retry_count, 2);
        assert_eq!(diagnostics[0].ts_event, UnixNanos::from(404_000_000));
    }
}

#[rstest]
#[case::submitted(false)]
#[case::pending_cancel(true)]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_retained_submission_survives_recovery_and_accepts_late_evidence(
    #[case] pending_cancel: bool,
    #[values(false, true)] client_retention: bool,
) {
    let config = ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 1,
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 1,
        open_check_open_only: false,
        submission_recovery_policy: if client_retention {
            SubmissionRecoveryPolicy::ResolveLocally
        } else {
            SubmissionRecoveryPolicy::RetainUnresolved
        },
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());
    let mut client = MockExecutionClient::new(vec![]);
    client.retain_unresolved_submissions = client_retention;
    assert!(ctx.manager.check_open_orders(&[&client]).await.is_empty());
    let client_order_id = ClientOrderId::from("O-RETAINED");
    let mut order = create_submitted_order(
        "O-RETAINED",
        test_instrument_id(),
        OrderSide::Buy,
        "1.0",
        "3000.00",
    );

    if pending_cancel {
        order
            .apply(OrderEventAny::PendingCancel(
                OrderPendingCancelSpec::builder()
                    .trader_id(order.trader_id())
                    .strategy_id(order.strategy_id())
                    .instrument_id(order.instrument_id())
                    .client_order_id(order.client_order_id())
                    .account_id(test_account_id())
                    .build(),
            ))
            .unwrap();
    }

    ctx.add_order(order.clone());
    ctx.manager
        .register_submission(order.init_event(), Some(test_client_id()));
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;

    let exhausted = ctx.manager.check_inflight_orders();
    assert_eq!(
        ctx.manager.take_submission_recovery_exhaustions(),
        vec![SubmissionRecoveryExhausted {
            trader_id: order.trader_id(),
            client_id: Some(test_client_id()),
            strategy_id: order.strategy_id(),
            instrument_id: order.instrument_id(),
            client_order_id,
            source: SubmissionRecoverySource::Inflight,
            retry_count: 1,
            ts_event: ctx.clock.borrow().timestamp_ns(),
        }]
    );
    let mut duplicate = order.init_event().clone();
    duplicate.strategy_id = StrategyId::from("OTHER-002");
    ctx.manager
        .register_submission(&duplicate, Some(ClientId::from("OTHER")));
    ctx.manager.register_inflight(client_order_id, UUID4::new());
    ctx.manager.clear_recon_tracking(&client_order_id, true);
    ctx.advance_both(dst::time::Duration::from_millis(200))
        .await;
    let missing = ctx.manager.check_open_orders(&[&client]).await;
    let repeated = ctx.manager.check_inflight_orders();
    let queries = ctx.manager.check_open_order_queries();
    let accepted =
        TestOrderEventStubs::accepted(&order, test_account_id(), VenueOrderId::from("V-LATE"));
    ctx.cache.borrow_mut().update_order(&accepted).unwrap();
    ctx.manager.confirm_submission_outcome(&client_order_id);

    assert!(exhausted.events.is_empty());
    assert!(exhausted.queries.is_empty());
    assert!(missing.is_empty());
    assert!(repeated.events.is_empty());
    assert!(repeated.queries.is_empty());
    assert!(queries.is_empty());
    assert!(
        ctx.manager
            .take_submission_recovery_exhaustions()
            .is_empty()
    );
    assert_eq!(client.order_report_query_count.get(), 0);
    assert_eq!(
        ctx.cache.borrow().order(&client_order_id).unwrap().status(),
        OrderStatus::Accepted
    );
}

#[rstest]
#[cfg_attr(
    not(all(feature = "simulation", madsim)),
    tokio::test(start_paused = true)
)]
#[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
async fn test_submission_retention_is_scoped_to_client(
    #[values(false, true)] explicit_client: bool,
) {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        inflight_threshold_ms: 100,
        inflight_max_retries: 1,
        submission_recovery_policy: SubmissionRecoveryPolicy::ResolveLocally,
        ..Default::default()
    });

    ctx.add_instrument(test_instrument());
    let mut protected = MockExecutionClient::new(Vec::new());
    protected.retain_unresolved_submissions = true;
    let ordinary_id = ClientId::from("ORDINARY");
    let ordinary = MockExecutionClient::for_venue(ordinary_id, test_venue(), Vec::new());
    assert!(
        ctx.manager
            .check_open_orders(&[&protected, &ordinary])
            .await
            .is_empty()
    );

    let protected_order = create_submitted_order(
        "O-PROTECTED",
        test_instrument_id(),
        OrderSide::Buy,
        "1.0",
        "3000.00",
    );
    let ordinary_order = create_submitted_order(
        "O-ORDINARY",
        test_instrument_id(),
        OrderSide::Buy,
        "2.0",
        "3001.00",
    );

    for (order, client_id) in [
        (&protected_order, test_client_id()),
        (&ordinary_order, ordinary_id),
    ] {
        ctx.add_order_with_client_id(order.clone(), client_id);
        ctx.manager
            .register_submission(order.init_event(), explicit_client.then_some(client_id));
    }

    ctx.advance_both(dst::time::Duration::from_millis(101))
        .await;
    let result = ctx.manager.check_inflight_orders();
    for event in &result.events {
        ctx.cache.borrow_mut().update_order(event).unwrap();
    }

    assert!(result.queries.is_empty());
    assert_eq!(result.events.len(), 1);

    let OrderEventAny::Rejected(rejected) = &result.events[0] else {
        panic!("Expected timeout rejection, was {:?}", result.events[0]);
    };

    assert_eq!(rejected.client_order_id, ordinary_order.client_order_id());
    assert_eq!(rejected.reason, "INFLIGHT_TIMEOUT");
    assert_eq!(
        ctx.get_order(&ordinary_order.client_order_id())
            .unwrap()
            .status(),
        OrderStatus::Rejected
    );
    assert_eq!(
        ctx.get_order(&protected_order.client_order_id())
            .unwrap()
            .status(),
        OrderStatus::Submitted
    );
    assert_eq!(
        ctx.manager.take_submission_recovery_exhaustions(),
        vec![SubmissionRecoveryExhausted {
            trader_id: protected_order.trader_id(),
            client_id: Some(test_client_id()),
            strategy_id: protected_order.strategy_id(),
            instrument_id: protected_order.instrument_id(),
            client_order_id: protected_order.client_order_id(),
            source: SubmissionRecoverySource::Inflight,
            retry_count: 1,
            ts_event: ctx.clock.borrow().timestamp_ns(),
        }]
    );
}

#[rstest]
#[case::resolve_locally(SubmissionRecoveryPolicy::ResolveLocally, false, 1)]
#[case::retain_unresolved(SubmissionRecoveryPolicy::RetainUnresolved, false, 1)]
#[case::retain_unresolved_multiple_checks(SubmissionRecoveryPolicy::RetainUnresolved, false, 3)]
#[case::client_required(SubmissionRecoveryPolicy::ResolveLocally, true, 1)]
#[tokio::test]
async fn test_check_open_orders_submitted_missing_at_venue_obeys_recovery_policy(
    #[case] policy: SubmissionRecoveryPolicy,
    #[case] client_retention: bool,
    #[case] budget: u32,
) {
    let config = ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: budget,
        open_check_open_only: false,
        submission_recovery_policy: policy,
        single_order_query_delay_ms: 0,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    ctx.add_instrument(test_instrument());

    let order = create_limit_order(
        "O-001",
        test_instrument_id(),
        OrderSide::Buy,
        "10.0",
        "100.0",
    );
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    ctx.add_order(order);
    ctx.cache.borrow_mut().update_order(&submitted).unwrap();

    // Neither bulk nor targeted reads establish the submission outcome
    let mut mock_client = MockExecutionClient::new(vec![]);
    mock_client.retain_unresolved_submissions = client_retention;
    let clients: Vec<&dyn ExecutionClient> = vec![&mock_client];

    for _ in 1..budget {
        assert!(ctx.manager.check_open_orders(&clients).await.is_empty());
        assert!(
            ctx.manager
                .take_submission_recovery_exhaustions()
                .is_empty()
        );
        assert_eq!(mock_client.order_report_query_count.get(), 0);
    }

    let events = ctx.manager.check_open_orders(&clients).await;

    let client_order_id = ClientOrderId::from("O-001");
    let order = ctx.get_order(&client_order_id).unwrap();

    let expected = if policy == SubmissionRecoveryPolicy::RetainUnresolved || client_retention {
        vec![SubmissionRecoveryExhausted {
            trader_id: order.trader_id(),
            client_id: Some(test_client_id()),
            strategy_id: order.strategy_id(),
            instrument_id: order.instrument_id(),
            client_order_id,
            source: SubmissionRecoverySource::MissingOrder,
            retry_count: budget,
            ts_event: ctx.clock.borrow().timestamp_ns(),
        }]
    } else {
        Vec::new()
    };

    assert_eq!(ctx.manager.take_submission_recovery_exhaustions(), expected);
    assert!(
        ctx.manager
            .take_submission_recovery_exhaustions()
            .is_empty()
    );

    if policy == SubmissionRecoveryPolicy::RetainUnresolved || client_retention {
        let queries_before = mock_client.order_report_query_count.get();
        let repeated = ctx.manager.check_open_orders(&clients).await;
        assert!(events.is_empty());
        assert!(repeated.is_empty());
        assert_eq!(mock_client.order_report_query_count.get(), queries_before);
        assert_eq!(
            ctx.cache
                .borrow()
                .order(&ClientOrderId::from("O-001"))
                .unwrap()
                .status(),
            OrderStatus::Submitted
        );
        return;
    }

    assert_eq!(events.len(), 1);
    assert_eq!(mock_client.order_report_query_count.get(), 1);

    if let OrderEventAny::Rejected(rejected) = &events[0] {
        assert_eq!(rejected.client_order_id, ClientOrderId::from("O-001"));
        assert_eq!(rejected.reason, "NOT_FOUND_AT_VENUE");
    } else {
        panic!("Expected OrderRejected event, was {:?}", events[0]);
    }
}

#[rstest]
#[case::accepted("accepted", 1)]
#[case::targeted_error("targeted_error", 1)]
#[case::wrong_identity("wrong_identity", 1)]
#[case::bulk_error("bulk_error", 0)]
#[case::open_only("open_only", 0)]
#[case::query_cap("query_cap", 0)]
#[tokio::test]
async fn test_submission_registry_missing_checks_do_not_exhaust_without_absence(
    #[case] scenario: &str,
    #[case] query_count: usize,
) {
    let mut ctx = TestContext::with_config(ExecutionManagerConfig {
        open_check_threshold_ns: DurationNanos::ZERO,
        open_check_missing_retries: 1,
        open_check_open_only: scenario == "open_only",
        max_single_order_queries_per_cycle: u32::from(scenario != "query_cap"),
        single_order_query_delay_ms: 0,
        submission_recovery_policy: SubmissionRecoveryPolicy::RetainUnresolved,
        ..Default::default()
    });

    ctx.add_instrument(test_instrument());
    let order = create_limit_order(
        "O-REGISTRY-MISSING",
        test_instrument_id(),
        OrderSide::Buy,
        "10.0",
        "100.0",
    );
    let client_order_id = order.client_order_id();
    let submitted = TestOrderEventStubs::submitted(&order, test_account_id());
    ctx.manager
        .register_submission(order.init_event(), Some(test_client_id()));
    ctx.add_order(order);
    ctx.cache.borrow_mut().update_order(&submitted).unwrap();

    let client = match scenario {
        "accepted" | "wrong_identity" => MockExecutionClient::new(Vec::new()).with_order_report(
            create_order_report(
                Some(if scenario == "accepted" {
                    client_order_id
                } else {
                    ClientOrderId::from("O-OTHER")
                }),
                VenueOrderId::from("V-REGISTRY-MISSING"),
                test_instrument_id(),
                OrderStatus::Accepted,
                Quantity::from("10.0"),
                Quantity::zero(1),
            )
            .with_price(Price::from("100.0")),
        ),
        "targeted_error" => MockExecutionClient::new(Vec::new()).with_failed_order_report(),
        "bulk_error" => MockExecutionClient::failing(test_client_id(), test_venue()),
        _ => MockExecutionClient::new(Vec::new()),
    };

    let events = ctx.manager.check_open_orders(&[&client]).await;

    assert_eq!(client.order_report_query_count.get(), query_count);

    if scenario == "accepted" {
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], OrderEventAny::Accepted(accepted)
            if accepted.client_order_id == client_order_id));
    } else {
        assert!(events.is_empty());
    }

    assert_eq!(
        ctx.get_order(&client_order_id).unwrap().status(),
        OrderStatus::Submitted
    );
    assert!(
        ctx.manager
            .take_submission_recovery_exhaustions()
            .is_empty()
    );
}

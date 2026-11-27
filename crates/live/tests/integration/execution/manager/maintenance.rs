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
fn test_config_default_values() {
    let config = ExecutionManagerConfig::default();

    assert_eq!(config.lookback_mins, Some(60));
    assert!(!config.filter_unclaimed_external);
    assert!(!config.filter_position_reports);
    assert!(config.generate_missing_orders);
    assert_eq!(config.inflight_threshold_ms, 5_000);
    assert_eq!(config.inflight_max_retries, 5);
    assert_eq!(
        config.submission_recovery_policy,
        SubmissionRecoveryPolicy::RetainUnresolved,
    );
}

#[rstest]
fn test_config_with_trader_id() {
    let trader_id = TraderId::from("TRADER-001");
    let config = ExecutionManagerConfig::default().with_trader_id(trader_id);

    assert_eq!(config.trader_id, trader_id);
}

const PURGE_MINUTE_NS: u64 = 60_000_000_000;

#[rstest]
#[case::disabled(None, None, None, PURGE_MINUTE_NS, [false; 3])]
#[case::immediate(Some(0), Some(0), Some(0), PURGE_MINUTE_NS, [true; 3])]
#[case::orders_before(Some(1), None, None, PURGE_MINUTE_NS - 1, [false; 3])]
#[case::orders_at(Some(1), None, None, PURGE_MINUTE_NS, [true, false, false])]
#[case::positions_before(None, Some(1), None, PURGE_MINUTE_NS - 1, [false; 3])]
#[case::positions_at(None, Some(1), None, PURGE_MINUTE_NS, [false, true, false])]
#[case::accounts_before(None, None, Some(1), PURGE_MINUTE_NS - 1, [false; 3])]
#[case::accounts_at(None, None, Some(1), PURGE_MINUTE_NS, [false, false, true])]
fn test_purge_operations_respect_retention_config(
    #[case] orders_mins: Option<u32>,
    #[case] positions_mins: Option<u32>,
    #[case] accounts_mins: Option<u32>,
    #[case] elapsed_ns: u64,
    #[case] purged: [bool; 3],
    #[values(false, true)] purge_from_database: bool,
) {
    let config = ExecutionManagerConfig {
        purge_closed_orders_buffer_mins: orders_mins,
        purge_closed_positions_buffer_mins: positions_mins,
        purge_account_events_lookback_mins: accounts_mins,
        purge_from_database,
        ..Default::default()
    };

    let mut ctx = TestContext::with_config(config);
    let instrument = test_instrument();
    ctx.add_instrument(instrument.clone());
    let order = create_submitted_order(
        "O-PURGE",
        instrument.id(),
        OrderSide::Buy,
        "1.000",
        "3000.00",
    );
    ctx.add_order(order.clone());
    let accepted =
        TestOrderEventStubs::accepted(&order, test_account_id(), VenueOrderId::from("V-PURGE"));
    let canceled = TestOrderEventStubs::canceled(
        &order,
        test_account_id(),
        Some(VenueOrderId::from("V-PURGE")),
    );
    ctx.cache.borrow_mut().update_order(&accepted).unwrap();
    ctx.cache.borrow_mut().update_order(&canceled).unwrap();
    let mut position = create_test_position(
        &instrument,
        PositionId::from("P-PURGE"),
        OrderSide::Buy,
        "2.000",
        "3000.00",
    );
    ctx.add_position(&position);
    let closing_order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument.id())
        .client_order_id(ClientOrderId::from("O-PURGE-POSITION-CLOSE"))
        .side(OrderSide::Sell)
        .quantity(position.quantity)
        .build();

    let OrderEventAny::Filled(fill) = OrderFilledTestBuilder::new(&closing_order, &instrument)
        .account_id(test_account_id())
        .position_id(position.id)
        .trade_id(TradeId::from("T-PURGE-CLOSE"))
        .last_px(Price::from("3000.00"))
        .build()
    else {
        panic!("expected a closing fill");
    };

    position.apply(&fill);
    ctx.cache.borrow_mut().update_position(&position).unwrap();
    let first = ctx
        .cache
        .borrow()
        .account(&test_account_id())
        .unwrap()
        .last_event()
        .unwrap();
    let mut second = first.clone();
    second.event_id = UUID4::new();
    second.ts_event = UnixNanos::from(1);
    second.ts_init = UnixNanos::from(2);
    ctx.cache
        .borrow_mut()
        .update_account_state(&second)
        .unwrap();
    ctx.advance_time(elapsed_ns);
    assert!(ctx.cache.borrow().is_order_closed(&order.client_order_id()));
    assert!(ctx.cache.borrow().is_position_closed(&position.id));

    let (database, database_control) = TestCacheDatabaseControl::create();
    ctx.cache.borrow_mut().set_database(Box::new(database));
    let mut expected_database_events = Vec::new();

    if purge_from_database {
        if purged[0] {
            expected_database_events.push(format!("delete_order:{}", order.client_order_id()));
        }

        if purged[1] {
            expected_database_events.push(format!("delete_position:{}", position.id));
        }

        if purged[2] {
            expected_database_events.push(format!(
                "delete_account_event:{}:{}",
                test_account_id(),
                first.event_id
            ));
        }
    }

    ctx.manager.purge_closed_orders();
    ctx.manager.purge_closed_positions();
    ctx.manager.purge_account_events();

    assert_eq!(database_control.events(), expected_database_events);
    let cache = ctx.cache.borrow();
    assert_eq!(
        cache
            .order(&order.client_order_id())
            .map(|order| order.status()),
        (!purged[0]).then_some(OrderStatus::Canceled)
    );
    assert_eq!(
        cache
            .position(&position.id)
            .map(|position| position.quantity),
        (!purged[1]).then_some(Quantity::zero(3))
    );

    let expected_events = if purged[2] {
        vec![second]
    } else {
        vec![first, second]
    };

    assert_eq!(
        serde_json::to_value(cache.account(&test_account_id()).unwrap().events()).unwrap(),
        serde_json::to_value(expected_events).unwrap()
    );
}

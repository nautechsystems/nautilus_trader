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

//! Integration tests for `ExecutionEventEmitter`.
//!
//! These tests exercise the emitter through the crate's public API, so they also pin which
//! methods are public. Internal state tests are in the in-module tests in emitter.rs.

use nautilus_common::messages::ExecutionEvent;
use nautilus_core::{UUID4, UnixNanos, time::AtomicTime};
use nautilus_live::ExecutionEventEmitter;
use nautilus_model::{
    enums::{AccountType, LiquiditySide, OrderSide, OrderType},
    events::{
        AccountState, OrderEventAny,
        order::spec::{OrderFilledSpec, OrderUpdatedSpec},
    },
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TradeId, TraderId,
        VenueOrderId,
    },
    orders::{OrderAny, OrderTestBuilder},
    types::{AccountBalance, Currency, Money, Price, Quantity},
};
use rstest::{fixture, rstest};

const TS_INIT: u64 = 987_654_321;
const TRADER_ID: &str = "TRADER-001";
const ACCOUNT_ID: &str = "SIM-001";

#[fixture]
fn emitter() -> ExecutionEventEmitter {
    let clock = Box::leak(Box::new(AtomicTime::new(false, UnixNanos::from(TS_INIT))));

    ExecutionEventEmitter::new(
        clock,
        TraderId::from(TRADER_ID),
        AccountId::from(ACCOUNT_ID),
        AccountType::Cash,
        None,
    )
}

fn create_account_state(ts_init: UnixNanos) -> AccountState {
    let usd = Currency::USD();

    AccountState::new(
        AccountId::from(ACCOUNT_ID),
        AccountType::Cash,
        vec![AccountBalance::new(
            Money::from("1500 USD"),
            Money::from("250 USD"),
            Money::from("1250 USD"),
        )],
        Vec::new(),
        true,
        UUID4::new(),
        UnixNanos::from(123_456_789),
        ts_init,
        Some(usd),
    )
}

#[rstest]
fn test_try_send_account_state_preserves_caller_built_state(mut emitter: ExecutionEventEmitter) {
    let ts_init = UnixNanos::from(789_123_456);
    let state = create_account_state(ts_init);

    let error = emitter.try_send_account_state(state.clone()).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Cannot send account state: sender not initialized"
    );

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    emitter.set_sender(tx);

    emitter.try_send_account_state(state.clone()).unwrap();

    // `AccountState::PartialEq` compares identity fields only, so pin each field.
    match rx.try_recv() {
        Ok(ExecutionEvent::Account(received)) => {
            assert_eq!(received.account_id, state.account_id);
            assert_eq!(received.account_type, state.account_type);
            assert_eq!(received.event_id, state.event_id);
            assert_eq!(received.balances, state.balances);
            assert_eq!(received.margins, state.margins);
            assert_eq!(received.is_reported, state.is_reported);
            assert_eq!(received.base_currency, state.base_currency);
            assert_eq!(received.ts_event, state.ts_event);
            assert_eq!(received.ts_init, ts_init);
            assert_eq!(received.info, state.info);
        }
        other => panic!("expected an account state event, was {other:?}"),
    }
}

#[rstest]
#[case::with_fee_and_position(
    Some(Money::from("0.37 USDT")),
    Some(PositionId::from("P-042")),
    LiquiditySide::Maker
)]
#[case::without_fee_or_position(None, None, LiquiditySide::Taker)]
fn test_emit_order_filled_preserves_fields(
    mut emitter: ExecutionEventEmitter,
    order: OrderAny,
    #[case] commission: Option<Money>,
    #[case] position_id: Option<PositionId>,
    #[case] liquidity_side: LiquiditySide,
) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    emitter.set_sender(tx);
    let venue_order_id = VenueOrderId::from("V-019");
    let trade_id = TradeId::from("T-023");
    let qty = Quantity::from("2.345");
    let price = Price::from("2345.67");
    let ts_event = UnixNanos::from(123_456_789);

    emitter.emit_order_filled(
        &order,
        venue_order_id,
        position_id,
        trade_id,
        qty,
        price,
        Currency::USDT(),
        commission,
        liquidity_side,
        ts_event,
    );

    let ExecutionEvent::Order(OrderEventAny::Filled(actual)) = rx.try_recv().unwrap() else {
        panic!("expected an order fill");
    };

    let expected = OrderFilledSpec::builder()
        .trader_id(TraderId::from(TRADER_ID))
        .strategy_id(StrategyId::from("STRATEGY-007"))
        .instrument_id(InstrumentId::from("ETHUSDT.BINANCE"))
        .client_order_id(ClientOrderId::from("O-011"))
        .venue_order_id(venue_order_id)
        .account_id(AccountId::from(ACCOUNT_ID))
        .trade_id(trade_id)
        .order_side(OrderSide::Sell)
        .order_type(OrderType::Limit)
        .last_qty(qty)
        .last_px(price)
        .currency(Currency::USDT())
        .liquidity_side(liquidity_side)
        .event_id(actual.event_id)
        .ts_event(ts_event)
        .ts_init(UnixNanos::from(TS_INIT))
        .maybe_position_id(position_id)
        .maybe_commission(commission)
        .build();

    assert_eq!(
        serde_json::to_value(actual).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    assert!(rx.try_recv().is_err());
}

#[rstest]
fn test_emit_order_updated_preserves_fields(mut emitter: ExecutionEventEmitter, order: OrderAny) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    emitter.set_sender(tx);
    let venue_order_id = VenueOrderId::from("V-029");
    let qty = Quantity::from("7.654");
    let price = Some(Price::from("2345.67"));
    let trigger_price = Some(Price::from("2301.23"));
    let protection_price = Some(Price::from("2299.99"));
    let ts_event = UnixNanos::from(123_456_789);

    emitter.emit_order_updated(
        &order,
        venue_order_id,
        qty,
        price,
        trigger_price,
        protection_price,
        ts_event,
    );

    let ExecutionEvent::Order(OrderEventAny::Updated(actual)) = rx.try_recv().unwrap() else {
        panic!("expected an order update");
    };

    let expected = OrderUpdatedSpec::builder()
        .trader_id(TraderId::from(TRADER_ID))
        .strategy_id(StrategyId::from("STRATEGY-007"))
        .instrument_id(InstrumentId::from("ETHUSDT.BINANCE"))
        .client_order_id(ClientOrderId::from("O-011"))
        .venue_order_id(venue_order_id)
        .account_id(AccountId::from(ACCOUNT_ID))
        .quantity(qty)
        .maybe_price(price)
        .maybe_trigger_price(trigger_price)
        .maybe_protection_price(protection_price)
        .event_id(actual.event_id)
        .ts_event(ts_event)
        .ts_init(UnixNanos::from(TS_INIT))
        .build();

    assert_eq!(
        serde_json::to_value(actual).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    assert!(rx.try_recv().is_err());
}

#[rstest]
fn test_try_emit_order_submitted_reports_sender_errors(
    mut emitter: ExecutionEventEmitter,
    order: OrderAny,
) {
    let uninitialized = emitter.try_emit_order_submitted(&order).unwrap_err();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    emitter.set_sender(tx);
    drop(rx);
    let closed = emitter.try_emit_order_submitted(&order).unwrap_err();

    assert_eq!(
        uninitialized.to_string(),
        "Cannot send order event: sender not initialized"
    );
    assert_eq!(
        closed.to_string(),
        "Failed to send order event: channel closed"
    );
}

#[fixture]
fn order() -> OrderAny {
    OrderTestBuilder::new(OrderType::Limit)
        .trader_id(TraderId::from("ORDER-TRADER-003"))
        .strategy_id(StrategyId::from("STRATEGY-007"))
        .instrument_id(InstrumentId::from("ETHUSDT.BINANCE"))
        .client_order_id(ClientOrderId::from("O-011"))
        .side(OrderSide::Sell)
        .quantity(Quantity::from("13.000"))
        .price(Price::from("2400.00"))
        .build()
}

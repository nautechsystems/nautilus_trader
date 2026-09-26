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

use nautilus_common::{clock::VirtualClock, msgbus::stubs::get_typed_into_message_saving_handler};
use nautilus_core::UnixNanos;
use nautilus_model::{
    accounts::CashAccount, identifiers::ClientOrderId, instruments::stubs::audusd_sim,
    orders::OrderTestBuilder, types::money::MONEY_RAW_MAX,
};
use rstest::{fixture, rstest};
use rust_decimal_macros::dec;

use super::*;

#[fixture]
fn engine() -> RiskEngine {
    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock = Rc::new(RefCell::new(VirtualClock::new()));
    let portfolio = Portfolio::new(
        Rc::clone(&clock) as Rc<RefCell<dyn Clock>>,
        Rc::clone(&cache),
        None,
    );
    RiskEngine::new(RiskEngineConfig::default(), portfolio, clock, cache)
}

#[rstest]
#[case::increase("7 USD", "3 USD", "4 USD")]
#[case::reduction("3 USD", "7 USD", "0 USD")]
#[case::equal("7 USD", "7 USD", "0 USD")]
#[case::negative_previous("7 USD", "-3 USD", "7 USD")]
#[case::negative_current("-7 USD", "3 USD", "0 USD")]
#[case::both_negative("-7 USD", "-3 USD", "0 USD")]
fn test_risk_increase_clamps_credits(
    engine: RiskEngine,
    #[case] current: &str,
    #[case] previous: &str,
    #[case] expected: &str,
) {
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(InstrumentId::from("AUD/USD.SIM"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from("100"))
        .build();
    let (handler, saved) = get_typed_into_message_saving_handler::<OrderEventAny>(None);
    msgbus::register_order_event_endpoint(MessagingSwitchboard::exec_engine_process(), handler);

    let increase = engine.check_risk_increase(
        RiskCheck::Submit,
        &order,
        Money::from(current),
        Money::from(previous),
    );

    assert_eq!(increase, Some(Money::from(expected)));
    assert!(saved.get_messages().is_empty());
}

#[rstest]
fn test_risk_increase_at_money_bounds(engine: RiskEngine) {
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(InstrumentId::from("AUD/USD.SIM"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from("100"))
        .build();
    let maximum = Money::from_raw(MONEY_RAW_MAX, Currency::USD());

    assert_eq!(
        engine.check_risk_increase(RiskCheck::Submit, &order, maximum, -maximum),
        Some(maximum)
    );
    assert_eq!(
        engine.check_risk_increase(RiskCheck::Submit, &order, -maximum, maximum),
        Some(Money::zero(Currency::USD()))
    );
}

#[rstest]
#[case::increase(false)]
#[case::cumulative(true)]
fn test_risk_arithmetic_rejects_incompatible_currency(
    engine: RiskEngine,
    #[case] cumulative: bool,
) {
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(InstrumentId::from("AUD/USD.SIM"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from("100"))
        .build();
    let (handler, saved) = get_typed_into_message_saving_handler::<OrderEventAny>(None);
    msgbus::register_order_event_endpoint(MessagingSwitchboard::exec_engine_process(), handler);
    let initial = Money::from("3 USD");
    let mut total = Some(initial);
    let incoming = Money::from("7 GBP");

    let accepted = if cumulative {
        engine.accumulate_notional(RiskCheck::Submit, &order, &mut total, incoming)
    } else {
        engine
            .check_risk_increase(RiskCheck::Submit, &order, incoming, initial)
            .is_some()
    };

    let detail = if cumulative {
        "cumulative notional exceeds Money bounds or has incompatible currency or scale"
    } else {
        "amendment risk increase exceeds Money bounds or has incompatible currency"
    };

    let events = saved.get_messages();
    assert!(!accepted);
    assert_eq!(total, Some(initial));
    assert_eq!(events.len(), 1);

    let OrderEventAny::Denied(event) = &events[0] else {
        panic!("Expected OrderDenied")
    };

    assert_eq!(event.trader_id, order.trader_id());
    assert_eq!(event.strategy_id, order.strategy_id());
    assert_eq!(event.instrument_id, order.instrument_id());
    assert_eq!(event.ts_event, UnixNanos::default());
    assert_eq!(event.ts_init, UnixNanos::default());
    assert_eq!(events[0].client_order_id(), order.client_order_id());
    assert_eq!(
        events[0].message(),
        Some(Ustr::from(
            &OrderDeniedReason::NotionalCalculationFailed {
                detail: detail.to_string()
            }
            .to_string()
        ))
    );
}

#[rstest]
fn test_cash_sell_accumulation_rejects_overflow(engine: RiskEngine) {
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(InstrumentId::from("AUD/USD.SIM"))
        .quantity(Quantity::from("100"))
        .side(OrderSide::Sell)
        .build();
    let (handler, saved) = get_typed_into_message_saving_handler::<OrderEventAny>(None);
    msgbus::register_order_event_endpoint(MessagingSwitchboard::exec_engine_process(), handler);
    let maximum = Money::from_raw(MONEY_RAW_MAX, Currency::USD());
    let quantity = Quantity::from_decimal_dp(maximum.as_decimal() * dec!(0.75), 0).unwrap();
    let initial = Some(Money::from_quantity(quantity, Currency::USD()).unwrap());
    let mut total = initial;
    let account = CashAccount::default();

    let accepted = engine.check_cash_sell_balance(
        RiskCheck::Submit,
        &account,
        true,
        &order,
        quantity,
        Currency::USD(),
        &mut total,
    );

    let events = saved.get_messages();
    assert!(!accepted);
    assert_eq!(total, initial);
    assert_eq!(events.len(), 1);

    let OrderEventAny::Denied(event) = &events[0] else {
        panic!("Expected OrderDenied")
    };

    assert_eq!(event.trader_id, order.trader_id());
    assert_eq!(event.strategy_id, order.strategy_id());
    assert_eq!(event.instrument_id, order.instrument_id());
    assert_eq!(event.ts_event, UnixNanos::default());
    assert_eq!(event.ts_init, UnixNanos::default());
    assert_eq!(events[0].client_order_id(), order.client_order_id());
    assert_eq!(
        events[0].message(),
        Some(Ustr::from(
            &OrderDeniedReason::NotionalCalculationFailed {
                detail:
                    "cumulative notional exceeds Money bounds or has incompatible currency or scale"
                        .to_string(),
            }
            .to_string()
        ))
    );
}

#[rstest]
fn test_submit_orders_reject_invalid_notional_limit(mut engine: RiskEngine) {
    let instrument = InstrumentAny::CurrencyPair(audusd_sim());
    engine.set_max_notional_per_order(instrument.id(), Decimal::MAX);

    let orders: Vec<_> = ["O-001", "O-002"]
        .into_iter()
        .map(|id| {
            OrderTestBuilder::new(OrderType::Limit)
                .instrument_id(instrument.id())
                .client_order_id(ClientOrderId::from(id))
                .side(OrderSide::Buy)
                .quantity(Quantity::from("100"))
                .price(Price::from("1.00000"))
                .build()
        })
        .collect();

    let (handler, saved) = get_typed_into_message_saving_handler::<OrderEventAny>(None);
    msgbus::register_order_event_endpoint(MessagingSwitchboard::exec_engine_process(), handler);

    let accepted = engine.check_orders_risk(&instrument, &orders, false, RiskCheck::Submit, None);

    let events = saved.get_messages();
    assert!(!accepted);
    assert_eq!(events.len(), 2);

    for (event, order) in events.iter().zip(&orders) {
        let OrderEventAny::Denied(event) = event else {
            panic!("Expected OrderDenied")
        };

        assert_eq!(event.trader_id, order.trader_id());
        assert_eq!(event.strategy_id, order.strategy_id());
        assert_eq!(event.instrument_id, instrument.id());
        assert_eq!(event.client_order_id, order.client_order_id());
        assert_eq!(event.ts_event, UnixNanos::default());
        assert_eq!(event.ts_init, UnixNanos::default());
        assert_eq!(
            event.reason,
            Ustr::from(
                &OrderDeniedReason::InvalidMaxNotionalPerOrder {
                    instrument_id: instrument.id(),
                    value: Decimal::MAX,
                }
                .to_string()
            )
        );
    }
}

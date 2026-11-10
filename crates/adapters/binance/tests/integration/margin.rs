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

use nautilus_binance::{
    common::parse::parse_usdm_instrument, futures::http::models::BinanceFuturesUsdExchangeInfo,
};
use nautilus_common::{
    messages::execution::{SubmitOrder, TradingCommand},
    msgbus::{
        self, stubs::get_typed_into_message_saving_handler, switchboard::MessagingSwitchboard,
    },
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_live::testing::ExecutionHarness;
use nautilus_model::{
    accounts::{AccountAny, MarginAccount},
    enums::{AccountType, OrderSide, OrderType},
    events::{AccountState, OrderDeniedReason, OrderEventAny},
    identifiers::{AccountId, ClientId, TraderId},
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderTestBuilder},
    types::{AccountBalance, Currency, Money, Price, Quantity},
};
use rstest::{fixture, rstest};
use rust_decimal::Decimal;

#[fixture]
fn instrument() -> InstrumentAny {
    let info: BinanceFuturesUsdExchangeInfo = serde_json::from_str(include_str!(
        "../../test_data/futures/http_json/exchange_info_usdm.json"
    ))
    .unwrap();
    parse_usdm_instrument(&info.symbols[0], UnixNanos::default(), UnixNanos::default()).unwrap()
}

#[rstest]
#[case::default(None, "1000 USDT")]
#[case::twenty(Some(20), "50 USDT")]
#[case::hundred(Some(100), "10 USDT")]
fn test_futures_initial_margin_risk_boundary(
    instrument: InstrumentAny,
    #[case] leverage: Option<u32>,
    #[case] expected: &str,
    #[values(-1_i64, 0, 1)] balance_ticks: i64,
    #[values(OrderSide::Buy, OrderSide::Sell)] side: OrderSide,
) {
    let required = Money::from(expected);
    let free = Money::from_decimal(
        required.as_decimal() + Decimal::new(balance_ticks, 8),
        Currency::USDT(),
    )
    .unwrap();
    let account_id = AccountId::from("BINANCE-001");

    let harness = ExecutionHarness::new(
        TraderId::from("TESTER-001"),
        ClientId::from("BINANCE"),
        account_id,
        instrument.clone(),
    );

    let balance =
        AccountBalance::from_total_and_free(free.as_decimal(), free.as_decimal(), free.currency)
            .unwrap();

    let state = AccountState::new(
        account_id,
        AccountType::Margin,
        vec![balance],
        vec![],
        true,
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        None,
    );
    let mut account = MarginAccount::new(state, true);
    if let Some(leverage) = leverage {
        account.set_leverage(instrument.id(), Decimal::from(leverage));
    }

    let quantity = Quantity::from("0.020");
    let price = Price::from("50000.00");
    let calculated = account
        .calculate_initial_margin(&instrument, quantity, price, None)
        .unwrap();
    harness
        .cache()
        .borrow_mut()
        .add_account(AccountAny::Margin(account))
        .unwrap();
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument.id())
        .side(side)
        .quantity(quantity)
        .price(price)
        .build();
    let submit = SubmitOrder::from_order(
        &order,
        order.trader_id(),
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
    );
    let command = TradingCommand::SubmitOrder(submit);
    harness
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    let (handler, events) = get_typed_into_message_saving_handler::<OrderEventAny>(None);
    msgbus::register_order_event_endpoint(MessagingSwitchboard::exec_engine_process(), handler);
    let (handler, commands) = get_typed_into_message_saving_handler::<TradingCommand>(None);
    msgbus::register_trading_command_endpoint(
        MessagingSwitchboard::exec_engine_queue_execute(),
        handler,
    );

    harness.risk_engine().borrow_mut().execute(command.clone());
    let events = events.get_messages();
    let commands = commands.get_messages();

    assert_eq!(calculated, required);

    if balance_ticks < 0 {
        assert_eq!(events.len(), 1);

        let OrderEventAny::Denied(event) = &events[0] else {
            panic!("Expected OrderDenied")
        };

        assert_eq!(event.trader_id, order.trader_id());
        assert_eq!(event.strategy_id, order.strategy_id());
        assert_eq!(event.client_order_id, order.client_order_id());
        assert_eq!(event.instrument_id, instrument.id());
        assert_eq!(event.ts_event, UnixNanos::default());
        assert_eq!(event.ts_init, UnixNanos::default());
        assert_eq!(event.causation_id, None);
        assert_eq!(
            event.reason,
            OrderDeniedReason::InitialMarginExceedsFreeBalance {
                free_balance: free,
                initial_margin: required
            }
            .to_string()
        );
        assert!(commands.is_empty());
    } else {
        assert!(events.is_empty());
        assert_eq!(commands, vec![command]);
    }
}

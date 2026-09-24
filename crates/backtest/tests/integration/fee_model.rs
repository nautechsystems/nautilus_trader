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

use std::{cell::RefCell, rc::Rc};

use nautilus_backtest::{
    config::SimulatedVenueConfig, exchange::SimulatedExchange,
    execution_client::BacktestExecutionClient,
};
use nautilus_common::{
    cache::Cache,
    clock::VirtualClock,
    messages::execution::{SubmitOrder, TradingCommand},
    msgbus::{
        self, MessagingSwitchboard,
        stubs::{TypedIntoMessageSavingHandler, get_typed_into_message_saving_handler},
    },
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_execution::models::fee::{FeeModelAny, MakerTakerFeeModel};
use nautilus_model::{
    data::QuoteTick,
    enums::{AccountType, BookType, LiquiditySide, OmsType, OrderSide, OrderType},
    events::{OrderEventAny, OrderFilled},
    fees::MakerTakerFeeRates,
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, Symbol, TraderId, Venue},
    instruments::{
        Instrument, InstrumentAny,
        stubs::{audusd_sim, default_fx_ccy, gbpusd_sim},
    },
    orders::{Order, OrderAny, OrderTestBuilder, stubs::TestOrderEventStubs},
    stubs::TestDefault,
    types::{Currency, Money, Price, Quantity},
};
use rstest::rstest;
use rust_decimal::Decimal;

fn get_exchange_with_fee_model(
    fee_model: FeeModelAny,
) -> (Rc<RefCell<Cache>>, Rc<RefCell<SimulatedExchange>>) {
    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock = Rc::new(RefCell::new(VirtualClock::new()));
    let config = SimulatedVenueConfig::builder()
        .venue(Venue::new("SIM"))
        .oms_type(OmsType::Netting)
        .account_type(AccountType::Margin)
        .book_type(BookType::L1_MBP)
        .starting_balances(vec![Money::new(1_000_000.0, Currency::USD())])
        .default_leverage(Decimal::ONE)
        .fee_model(fee_model.into())
        .build()
        .unwrap();

    let exchange = Rc::new(RefCell::new(
        SimulatedExchange::new(config, cache.clone(), clock).unwrap(),
    ));
    SimulatedExchange::register_spread_quote_endpoint(&exchange);

    let exec_clock = VirtualClock::new();

    let execution_client = BacktestExecutionClient::new(
        TraderId::test_default(),
        AccountId::test_default(),
        &exchange,
        cache.clone(),
        Rc::new(RefCell::new(exec_clock)),
        None,
        None,
    );
    exchange
        .borrow_mut()
        .register_client(Rc::new(execution_client));

    (cache, exchange)
}

fn register_order_event_saving_handler() -> TypedIntoMessageSavingHandler<OrderEventAny> {
    let (handler, saving_handler) = get_typed_into_message_saving_handler::<OrderEventAny>(None);
    msgbus::register_order_event_endpoint(MessagingSwitchboard::exec_engine_process(), handler);
    saving_handler
}

fn add_quote(
    exchange: &Rc<RefCell<SimulatedExchange>>,
    cache: &Rc<RefCell<Cache>>,
    instrument_id: InstrumentId,
    bid: &str,
    ask: &str,
) {
    let quote = QuoteTick::new(
        instrument_id,
        Price::from(bid),
        Price::from(ask),
        Quantity::from("1000000"),
        Quantity::from("1000000"),
        UnixNanos::from(1),
        UnixNanos::from(1),
    );
    cache.borrow_mut().add_quote(quote).unwrap();
    exchange.borrow_mut().process_quote_tick(&quote).unwrap();
}

fn submit_market_buy(
    exchange: &Rc<RefCell<SimulatedExchange>>,
    cache: &Rc<RefCell<Cache>>,
    instrument_id: InstrumentId,
    client_order_id: &str,
    quantity: &str,
    ts_init: UnixNanos,
) -> OrderAny {
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument_id)
        .client_order_id(ClientOrderId::new(client_order_id))
        .side(OrderSide::Buy)
        .quantity(Quantity::from(quantity))
        .build();
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    cache
        .borrow_mut()
        .update_order(&TestOrderEventStubs::submitted(
            &order,
            AccountId::test_default(),
        ))
        .unwrap();
    let command = TradingCommand::SubmitOrder(SubmitOrder::new(
        TraderId::test_default(),
        None,
        StrategyId::test_default(),
        instrument_id,
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None,
        UUID4::default(),
        ts_init,
        None,
    ));
    exchange.borrow_mut().send(command);
    exchange.borrow_mut().process(ts_init);
    order
}

fn matching_fill(messages: &[OrderEventAny], client_order_id: ClientOrderId) -> &OrderFilled {
    messages
        .iter()
        .find_map(|event| match event {
            OrderEventAny::Filled(fill) if fill.client_order_id == client_order_id => Some(fill),
            _ => None,
        })
        .expect("Expected order fill")
}

#[rstest]
fn test_account_owned_schedule_override_applies_to_fill() {
    let saving_handler = register_order_event_saving_handler();
    let instrument = InstrumentAny::CurrencyPair(audusd_sim());
    let mut fee_model = MakerTakerFeeModel::new(Decimal::new(2, 3), Decimal::new(2, 3));
    fee_model.set_override(
        instrument.id(),
        MakerTakerFeeRates::new(Decimal::new(5, 4), Decimal::new(5, 4)),
    );
    let (cache, exchange) = get_exchange_with_fee_model(FeeModelAny::MakerTaker(fee_model));
    exchange
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();
    add_quote(&exchange, &cache, instrument.id(), "0.80000", "0.80010");

    let order = submit_market_buy(
        &exchange,
        &cache,
        instrument.id(),
        "O-FEE-OVERRIDE",
        "1000",
        UnixNanos::from(2),
    );

    let messages = saving_handler.get_messages();
    let fill = matching_fill(&messages, order.client_order_id());
    assert_eq!(fill.liquidity_side, LiquiditySide::Taker);
    // Override taker rate (0.0005) applies: 1000 * 0.80010 * 0.0005 = 0.40 USD.
    // The schedule default (0.002) would give 1.60 USD and the instrument's own
    // 0.00002 fee would give 0.02 USD.
    assert_eq!(fill.commission, Some(Money::from("0.40 USD")));
}

#[rstest]
fn test_fee_overrides_isolated_per_instrument() {
    let saving_handler = register_order_event_saving_handler();
    let audusd = InstrumentAny::CurrencyPair(audusd_sim());
    let gbpusd = InstrumentAny::CurrencyPair(gbpusd_sim());
    let eurusd = InstrumentAny::CurrencyPair(default_fx_ccy(
        Symbol::from("EUR/USD"),
        Some(Venue::new("SIM")),
    ));
    let mut fee_model = MakerTakerFeeModel::new(Decimal::new(1, 3), Decimal::new(1, 3));
    fee_model.set_override(
        audusd.id(),
        MakerTakerFeeRates::new(Decimal::new(5, 4), Decimal::new(5, 4)),
    );
    fee_model.set_override(
        gbpusd.id(),
        MakerTakerFeeRates::new(Decimal::new(3, 3), Decimal::new(3, 3)),
    );
    let (cache, exchange) = get_exchange_with_fee_model(FeeModelAny::MakerTaker(fee_model));
    for instrument in [&audusd, &gbpusd, &eurusd] {
        exchange
            .borrow_mut()
            .add_instrument(instrument.clone())
            .unwrap();
    }

    add_quote(&exchange, &cache, audusd.id(), "0.80000", "0.80010");
    add_quote(&exchange, &cache, gbpusd.id(), "1.27000", "1.27010");
    add_quote(&exchange, &cache, eurusd.id(), "1.09000", "1.09010");

    let aud_order = submit_market_buy(
        &exchange,
        &cache,
        audusd.id(),
        "O-FEE-AUD",
        "1000",
        UnixNanos::from(2),
    );
    let gbp_order = submit_market_buy(
        &exchange,
        &cache,
        gbpusd.id(),
        "O-FEE-GBP",
        "1000",
        UnixNanos::from(3),
    );
    let eur_order = submit_market_buy(
        &exchange,
        &cache,
        eurusd.id(),
        "O-FEE-EUR",
        "1000",
        UnixNanos::from(4),
    );

    let messages = saving_handler.get_messages();
    let aud_fill = matching_fill(&messages, aud_order.client_order_id());
    let gbp_fill = matching_fill(&messages, gbp_order.client_order_id());
    let eur_fill = matching_fill(&messages, eur_order.client_order_id());

    // Each instrument resolves its own rates with no cross-talk: overrides for
    // AUD (0.0005) and GBP (0.003), schedule default (0.001) for EUR.
    assert_eq!(aud_fill.commission, Some(Money::from("0.40 USD")));
    assert_eq!(gbp_fill.commission, Some(Money::from("3.81 USD")));
    assert_eq!(eur_fill.commission, Some(Money::from("1.09 USD")));
}

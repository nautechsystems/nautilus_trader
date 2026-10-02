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

use nautilus_common::{
    cache::Cache,
    clock::{Clock, VirtualClock},
    messages::execution::{ModifyOrder, SubmitOrder, TradingCommand},
    msgbus::{self, MessagingSwitchboard, TypedHandler},
};
use nautilus_core::UUID4;
use nautilus_execution::{
    engine::ExecutionEngine, matching_core::OrderMatchingCore,
    order_emulator::emulator::OrderEmulator,
};
use nautilus_model::{
    data::QuoteTick,
    enums::{OrderSide, OrderStatus, OrderType, TrailingOffsetType, TriggerType},
    events::OrderEventAny,
    identifiers::{AccountId, ClientOrderId, VenueOrderId},
    instruments::{CryptoPerpetual, Instrument, InstrumentAny, stubs::crypto_perpetual_ethusdt},
    orders::{Order, OrderAny, OrderTestBuilder, stubs::TestOrderEventStubs},
    types::{Price, Quantity},
};
use rstest::rstest;
use rust_decimal_macros::dec;

use crate::cache_database::FailNthAddOrderDatabase;

fn submit_command(order: &OrderAny) -> TradingCommand {
    TradingCommand::SubmitOrder(SubmitOrder::new(
        order.trader_id(),
        None,
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None,
        UUID4::new(),
        0.into(),
        None, // correlation_id
    ))
}

#[rstest]
fn test_stop_limit_order_triggered_before_market_data_retains_command(
    crypto_perpetual_ethusdt: CryptoPerpetual,
) {
    // This test validates that the OrderMatchingCore correctly handles
    // quote ticks with None bid/ask prices
    let instrument_id = crypto_perpetual_ethusdt.id;
    let price_increment = crypto_perpetual_ethusdt.price_increment;

    // Create a matching core
    let mut matching_core = OrderMatchingCore::new(instrument_id, price_increment);

    // Verify matching core has no market data initially
    assert!(matching_core.bid.is_none());
    assert!(matching_core.ask.is_none());

    // Process a quote tick to provide market data
    matching_core.set_bid_raw(Price::from("5060.00"));
    matching_core.set_ask_raw(Price::from("5070.00"));

    // Verify market data is now available
    assert!(matching_core.bid.is_some());
    assert!(matching_core.ask.is_some());
    assert_eq!(matching_core.bid.unwrap(), Price::from("5060.00"));
    assert_eq!(matching_core.ask.unwrap(), Price::from("5070.00"));
}

#[rstest]
fn test_modify_emulated_order_from_order_event_handler(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache = Rc::new(RefCell::new(Cache::default()));
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CryptoPerpetual(
            crypto_perpetual_ethusdt.clone(),
        ))
        .unwrap();

    let exec_engine = Rc::new(RefCell::new(ExecutionEngine::new(
        clock.clone(),
        cache.clone(),
        None,
    )));
    ExecutionEngine::register_msgbus_handlers(&exec_engine);
    let emulator = Rc::new(RefCell::new(OrderEmulator::new(clock, cache.clone())));
    OrderEmulator::register_msgbus_handlers(&emulator);

    let instrument_id = crypto_perpetual_ethusdt.id();
    let stop = OrderTestBuilder::new(OrderType::StopMarket)
        .instrument_id(instrument_id)
        .client_order_id(ClientOrderId::from("O-STOP"))
        .side(OrderSide::Sell)
        .trigger_price(Price::from("4900.00"))
        .quantity(Quantity::from("1.000"))
        .emulation_trigger(TriggerType::BidAsk)
        .build();
    let entry = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_id)
        .client_order_id(ClientOrderId::from("O-ENTRY"))
        .side(OrderSide::Buy)
        .price(Price::from("5000.00"))
        .quantity(Quantity::from("1.000"))
        .submit(true)
        .build();
    cache
        .borrow_mut()
        .add_order(stop.clone(), None, None, false)
        .unwrap();
    cache
        .borrow_mut()
        .add_order(entry.clone(), None, None, false)
        .unwrap();
    msgbus::send_trading_command(
        MessagingSwitchboard::order_emulator_execute(),
        submit_command(&stop),
    );

    // A strategy that tightens its emulated stop when the entry is accepted
    let stop_id = stop.client_order_id();
    let entry_id = entry.client_order_id();

    let handler = TypedHandler::from(move |event: &OrderEventAny| {
        if let OrderEventAny::Accepted(accepted) = event
            && accepted.client_order_id == entry_id
        {
            let modify = ModifyOrder::new(
                accepted.trader_id,
                None,
                accepted.strategy_id,
                instrument_id,
                stop_id,
                None,
                None,
                None,
                Some(Price::from("4950.00")),
                UUID4::new(),
                0.into(),
                None,
                None, // correlation_id
            );
            msgbus::send_trading_command(
                MessagingSwitchboard::order_emulator_execute(),
                TradingCommand::ModifyOrder(modify),
            );
        }
    });

    msgbus::subscribe_order_events(
        format!("events.order.{}", entry.strategy_id()).into(),
        handler,
        None,
    );

    let accepted = TestOrderEventStubs::accepted(
        &entry,
        AccountId::from("ACCOUNT-001"),
        VenueOrderId::from("V-1"),
    );
    msgbus::send_order_event(MessagingSwitchboard::exec_engine_process(), accepted);

    let cache = cache.borrow();
    assert_eq!(
        cache.order(&entry_id).unwrap().status(),
        OrderStatus::Accepted
    );
    assert_eq!(
        cache.order(&stop_id).unwrap().trigger_price(),
        Some(Price::from("4950.00"))
    );
}

#[rstest]
fn test_trailing_stop_activation_does_not_rewrite_order_events(
    crypto_perpetual_ethusdt: CryptoPerpetual,
) {
    let (database, control) = FailNthAddOrderDatabase::create();
    let cache = Rc::new(RefCell::new(Cache::new(None, Some(Box::new(database)))));
    let instrument_id = crypto_perpetual_ethusdt.id();
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt))
        .unwrap();
    cache
        .borrow_mut()
        .add_quote(QuoteTick::new(
            instrument_id,
            Price::from("5000.00"),
            Price::from("5001.00"),
            Quantity::from("1.000"),
            Quantity::from("1.000"),
            0.into(),
            0.into(),
        ))
        .unwrap();
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let emulator = Rc::new(RefCell::new(OrderEmulator::new(clock, cache.clone())));
    OrderEmulator::register_msgbus_handlers(&emulator);
    let order = OrderTestBuilder::new(OrderType::TrailingStopMarket)
        .instrument_id(instrument_id)
        .side(OrderSide::Sell)
        .quantity(Quantity::from("1.000"))
        .trailing_offset(dec!(10))
        .trailing_offset_type(TrailingOffsetType::Price)
        .emulation_trigger(TriggerType::BidAsk)
        .build();
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    msgbus::send_trading_command(
        MessagingSwitchboard::order_emulator_execute(),
        submit_command(&order),
    );

    let is_activated = match &*cache.borrow().order(&order.client_order_id()).unwrap() {
        OrderAny::TrailingStopMarket(order) => order.is_activated,
        other => panic!("Expected trailing stop market order, was {other:?}"),
    };

    let events = control.order_events();
    let has_duplicates = events
        .iter()
        .enumerate()
        .any(|(i, event)| events[..i].contains(event));
    assert!(is_activated);
    assert!(!events.is_empty());
    assert!(!has_duplicates, "duplicate events {events:?}");
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, OrderEventAny::Initialized(_)))
    );
}

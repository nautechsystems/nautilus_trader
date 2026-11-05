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
    actor::DataActor,
    cache::Cache,
    clock::VirtualClock,
    component::Component,
    messages::{
        data::{DataCommand, SubscribeCommand, UnsubscribeCommand},
        execution::TradingCommand,
    },
    msgbus::{
        self, MessageBus, MessagingSwitchboard, get_message_bus,
        stubs::{TypedIntoMessageSavingHandler, get_typed_into_message_saving_handler},
        switchboard::get_book_deltas_topic,
    },
};
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{BookOrder, OrderBookDelta, OrderBookDeltas},
    enums::{BookAction, BookType, OmsType, OrderSide, OrderType, TimeInForce},
    events::{
        OrderEventAny,
        order::spec::{OrderAcceptedSpec, OrderFilledSpec, OrderSubmittedSpec},
    },
    identifiers::{ClientOrderId, InstrumentId, PositionId, StrategyId, TraderId},
    instruments::{Instrument, InstrumentAny, stubs::currency_pair_btcusdt},
    orderbook::OrderBook,
    orders::{Order, builder::OrderTestBuilder},
    position::Position,
    types::{Money, Price, Quantity},
};
use nautilus_portfolio::portfolio::Portfolio;
use rstest::rstest;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use super::{OrderBookImbalance, OrderBookImbalanceConfig};
use crate::strategy::Strategy;

fn config() -> OrderBookImbalanceConfig {
    OrderBookImbalanceConfig::builder()
        .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
        .max_trade_size(Quantity::from("0.1234567"))
        .trigger_min_size(dec!(10))
        .build()
}

fn register_strategy(
    config: OrderBookImbalanceConfig,
) -> (
    OrderBookImbalance,
    Rc<RefCell<VirtualClock>>,
    Rc<RefCell<Cache>>,
) {
    *get_message_bus().borrow_mut() = MessageBus::default();
    let mut strategy = OrderBookImbalance::from_config(config).unwrap();
    let clock = Rc::new(RefCell::new(VirtualClock::new()));
    let cache = Rc::new(RefCell::new(Cache::default()));

    let portfolio = Rc::new(RefCell::new(Portfolio::new(
        clock.clone(),
        cache.clone(),
        None,
    )));

    let instrument = InstrumentAny::CurrencyPair(currency_pair_btcusdt());
    cache.borrow_mut().add_instrument(instrument).unwrap();
    strategy
        .core
        .register(
            TraderId::from("TESTER-001"),
            clock.clone(),
            cache.clone(),
            portfolio,
        )
        .unwrap();
    strategy.initialize().unwrap();
    (strategy, clock, cache)
}

fn book(cache: &Rc<RefCell<Cache>>, book_type: BookType, bid: &str, ask: &str) -> OrderBookDeltas {
    let instrument_id = config().instrument_id;

    let deltas = OrderBookDeltas::new(
        instrument_id,
        vec![
            OrderBookDelta::new(
                instrument_id,
                if bid == "0" {
                    BookAction::Delete
                } else {
                    BookAction::Add
                },
                BookOrder::new(
                    OrderSide::Buy,
                    Price::from("100.00"),
                    Quantity::from(bid),
                    1,
                ),
                0,
                1,
                UnixNanos::default(),
                UnixNanos::default(),
            ),
            OrderBookDelta::new(
                instrument_id,
                if ask == "0" {
                    BookAction::Delete
                } else {
                    BookAction::Add
                },
                BookOrder::new(
                    OrderSide::Sell,
                    Price::from("101.00"),
                    Quantity::from(ask),
                    2,
                ),
                0,
                2,
                UnixNanos::default(),
                UnixNanos::default(),
            ),
        ],
    );

    let mut book = OrderBook::new(instrument_id, book_type);
    book.apply_deltas(&deltas).unwrap();
    cache.borrow_mut().add_order_book(book).unwrap();
    deltas
}

fn trading_commands() -> TypedIntoMessageSavingHandler<TradingCommand> {
    let (handler, saver) = get_typed_into_message_saving_handler(None);
    msgbus::register_trading_command_endpoint(
        MessagingSwitchboard::risk_engine_queue_execute(),
        handler.clone(),
    );
    msgbus::register_trading_command_endpoint(
        MessagingSwitchboard::exec_engine_queue_execute(),
        handler,
    );
    saver
}

#[rstest]
#[case("20", "1", Some(OrderSide::Buy), "0.123456")]
#[case("1", "20", Some(OrderSide::Sell), "0.123456")]
#[case("20", "0.010000", Some(OrderSide::Buy), "0.010000")]
#[case("10", "1", None, "0")]
#[case("20", "4", None, "0")]
#[case("20", "4.000001", None, "0")]
#[case("20", "3.999999", Some(OrderSide::Buy), "0.123456")]
#[case("0", "20", None, "0")]
#[case("20", "0", None, "0")]
fn test_signal_boundaries_and_fok_fields(
    #[case] bid: &str,
    #[case] ask: &str,
    #[case] side: Option<OrderSide>,
    #[case] qty: &str,
) {
    let (mut strategy, _, cache) = register_strategy(config());
    let commands = trading_commands();
    strategy.start().unwrap();
    let deltas = book(&cache, BookType::L2_MBP, bid, ask);
    strategy.on_book_deltas(&deltas).unwrap();
    let messages = commands.get_messages();
    assert_eq!(messages.len(), usize::from(side.is_some()));
    assert_eq!(strategy.state.signal_count, u64::from(side.is_some()));

    if let Some(side) = side {
        let TradingCommand::SubmitOrder(command) = &messages[0] else {
            panic!("expected submit order");
        };

        let order = cache
            .borrow()
            .order_owned(&command.client_order_id)
            .unwrap();
        assert_eq!(order.instrument_id(), strategy.config.instrument_id);
        assert_eq!(order.strategy_id(), strategy.strategy_id().unwrap());
        assert_eq!(order.order_type(), OrderType::Limit);
        assert_eq!(order.order_side(), side);
        assert_eq!(order.quantity(), Quantity::from(qty));
        assert_eq!(
            order.price(),
            Some(Price::from(if side == OrderSide::Buy {
                "101.00"
            } else {
                "100.00"
            }))
        );
        assert_eq!(order.time_in_force(), TimeInForce::Fok);
        assert!(!order.is_post_only());
        assert!(!order.is_reduce_only());
        assert_eq!(strategy.state.last_trigger_ns, Some(UnixNanos::default()));
    }
}

#[rstest]
fn test_zero_quantized_cap_does_not_signal() {
    let mut config = config();
    config.max_trade_size = Quantity::from("0.0000001");
    let (mut strategy, _, cache) = register_strategy(config);
    let commands = trading_commands();
    strategy.start().unwrap();
    let deltas = book(&cache, BookType::L2_MBP, "20", "1");
    strategy.on_book_deltas(&deltas).unwrap();
    assert!(commands.get_messages().is_empty());
    assert_eq!(strategy.state.signal_count, 0);
    assert_eq!(strategy.state.last_trigger_ns, None);
}

#[rstest]
fn test_trigger_spacing_at_zero_and_interval_boundary() {
    let mut config = config();
    config.dry_run = true;
    let (mut strategy, clock, cache) = register_strategy(config);
    strategy.start().unwrap();
    let deltas = book(&cache, BookType::L2_MBP, "20", "1");

    for (now, expected) in [(0, 1), (0, 1), (999_999_999, 1), (1_000_000_000, 2)] {
        clock.borrow_mut().set_time(now.into());
        strategy.on_book_deltas(&deltas).unwrap();
        assert_eq!(strategy.state.signal_count, expected);
    }

    assert_eq!(strategy.state.update_count, 4);
    assert_eq!(
        cache
            .borrow()
            .order_book(&strategy.config.instrument_id)
            .unwrap()
            .best_bid_size(),
        Some(Quantity::from("20"))
    );
}

#[rstest]
#[case(true, 0)]
#[case(false, 1)]
fn test_inflight_filter_uses_runtime_strategy_id(#[case] own_strategy: bool, #[case] signals: u64) {
    let mut config = config();
    config.base.order_id_tag = Some("123".to_string());
    let (mut strategy, _, cache) = register_strategy(config);
    let commands = trading_commands();
    strategy.start().unwrap();
    let strategy_id = if own_strategy {
        strategy.strategy_id().unwrap()
    } else {
        StrategyId::from("OTHER-999")
    };

    let order = OrderTestBuilder::new(OrderType::Limit)
        .strategy_id(strategy_id)
        .instrument_id(strategy.config.instrument_id)
        .client_order_id(ClientOrderId::from("O-INFLIGHT"))
        .side(OrderSide::Buy)
        .price(Price::from("99.00"))
        .quantity(Quantity::from("1.000000"))
        .build();
    let submitted = OrderSubmittedSpec::builder()
        .trader_id(order.trader_id())
        .strategy_id(strategy_id)
        .instrument_id(order.instrument_id())
        .client_order_id(order.client_order_id())
        .build();
    cache
        .borrow_mut()
        .add_order(order, None, None, false)
        .unwrap();
    cache
        .borrow_mut()
        .update_order(&OrderEventAny::Submitted(submitted))
        .unwrap();
    let deltas = book(&cache, BookType::L2_MBP, "20", "1");
    strategy.on_book_deltas(&deltas).unwrap();
    assert_eq!(strategy.state.signal_count, signals);
    assert_eq!(commands.get_messages().len(), signals as usize);
}

#[rstest]
#[case(1, false, BookType::L2_MBP)]
#[case(2, false, BookType::L2_MBP)]
#[case(2, true, BookType::L3_MBO)]
fn test_stop_resume_reset_subscriptions(
    #[case] cycles: u64,
    #[case] resume: bool,
    #[case] book_type: BookType,
) {
    let mut config = config();
    config.dry_run = true;
    config.book_type = book_type;
    let (mut strategy, _, cache) = register_strategy(config);
    let (handler, saver) = get_typed_into_message_saving_handler::<DataCommand>(None);
    msgbus::register_data_command_endpoint(
        MessagingSwitchboard::data_engine_queue_execute(),
        handler,
    );
    let id = strategy.config.instrument_id;

    for cycle in 0..cycles {
        if cycle > 0 && !resume {
            strategy.reset().unwrap();
        }

        saver.clear();

        if cycle > 0 && resume {
            strategy.resume().unwrap();
        } else {
            strategy.start().unwrap();
        }

        assert_eq!(
            msgbus::subscriber_count_deltas(get_book_deltas_topic(id)),
            1
        );
        let deltas = book(&cache, book_type, "20", "1");
        strategy.on_book_deltas(&deltas).unwrap();
        Component::stop(&mut strategy).unwrap();
        let commands = saver.get_messages();
        assert_eq!(commands.len(), 2);

        let DataCommand::Subscribe(SubscribeCommand::BookDeltas(subscribe)) = &commands[0] else {
            panic!("expected subscribe");
        };

        let DataCommand::Unsubscribe(UnsubscribeCommand::BookDeltas(unsubscribe)) = &commands[1]
        else {
            panic!("expected unsubscribe");
        };

        assert_eq!(subscribe.instrument_ids, vec![id]);
        assert_eq!(subscribe.book_type, book_type);
        assert_eq!(subscribe.depth, None);
        assert_eq!(subscribe.client_id, None);
        assert_eq!(subscribe.venue, Some(id.venue));
        assert!(subscribe.managed);
        assert_eq!(subscribe.ts_init, UnixNanos::default());
        assert_eq!(subscribe.correlation_id, None);
        assert_eq!(subscribe.params, None);
        assert_eq!(unsubscribe.instrument_ids, subscribe.instrument_ids);
        assert_eq!(unsubscribe.client_id, subscribe.client_id);
        assert_eq!(unsubscribe.venue, subscribe.venue);
        assert_eq!(unsubscribe.ts_init, UnixNanos::default());
        assert_eq!(unsubscribe.correlation_id, Some(subscribe.command_id));
        assert_eq!(unsubscribe.params, subscribe.params);
        assert_eq!(
            strategy.state.update_count,
            if resume { cycle + 1 } else { 1 }
        );
        assert_eq!(strategy.state.signal_count, 1);
        assert!(strategy.is_stopped());
        assert_eq!(
            msgbus::subscriber_count_deltas(get_book_deltas_topic(id)),
            0
        );
    }

    strategy.reset().unwrap();
    assert_eq!(strategy.state.size_precision, None);
    assert_eq!(strategy.state.last_trigger_ns, None);
    assert_eq!(strategy.state.update_count, 0);
    assert_eq!(strategy.state.signal_count, 0);
    assert!(strategy.is_ready());
}

#[rstest]
fn test_dry_run_stop_with_open_orders_and_position_sends_no_commands() {
    let mut config = config();
    config.dry_run = true;
    let (mut strategy, _, cache) = register_strategy(config);
    strategy.start().unwrap();
    let instrument = InstrumentAny::CurrencyPair(currency_pair_btcusdt());
    let fill = OrderFilledSpec::builder()
        .strategy_id(strategy.strategy_id().unwrap())
        .instrument_id(instrument.id())
        .position_id(PositionId::from("P-DRY"))
        .last_qty(Quantity::from("2.000000"))
        .last_px(Price::from("100.00"))
        .commission(Money::from("0 USDT"))
        .build();
    let position = Position::new(&instrument, fill);
    cache
        .borrow_mut()
        .add_position(&position, OmsType::Hedging)
        .unwrap();
    let order = OrderTestBuilder::new(OrderType::Limit)
        .strategy_id(strategy.strategy_id().unwrap())
        .instrument_id(instrument.id())
        .client_order_id(ClientOrderId::from("O-DRY"))
        .side(OrderSide::Buy)
        .price(Price::from("99.00"))
        .quantity(Quantity::from("1.000000"))
        .build();
    let submitted = OrderSubmittedSpec::builder()
        .trader_id(order.trader_id())
        .strategy_id(order.strategy_id())
        .instrument_id(order.instrument_id())
        .client_order_id(order.client_order_id())
        .build();
    let accepted = OrderAcceptedSpec::builder()
        .trader_id(order.trader_id())
        .strategy_id(order.strategy_id())
        .instrument_id(order.instrument_id())
        .client_order_id(order.client_order_id())
        .build();
    cache
        .borrow_mut()
        .add_order(order, None, None, false)
        .unwrap();
    cache
        .borrow_mut()
        .update_order(&OrderEventAny::Submitted(submitted))
        .unwrap();
    cache
        .borrow_mut()
        .update_order(&OrderEventAny::Accepted(accepted))
        .unwrap();
    let commands = trading_commands();
    let deltas = book(&cache, BookType::L2_MBP, "20", "1");
    strategy.on_book_deltas(&deltas).unwrap();
    Component::stop(&mut strategy).unwrap();
    assert_eq!(strategy.state.signal_count, 1);
    assert!(commands.get_messages().is_empty());
    assert_eq!(
        cache
            .borrow()
            .orders_open_count(None, None, None, None, None),
        1
    );
    assert_eq!(
        cache
            .borrow()
            .positions_open_count(None, None, None, None, None),
        1
    );
}

#[rstest]
fn test_l3_uses_first_order_at_best_price() {
    let mut config = config();
    config.book_type = BookType::L3_MBO;
    let (mut strategy, _, cache) = register_strategy(config);
    let commands = trading_commands();
    strategy.start().unwrap();
    let deltas = book(&cache, BookType::L3_MBO, "1", "20");

    let extra = BookOrder::new(
        OrderSide::Buy,
        Price::from("100.00"),
        Quantity::from("100"),
        3,
    );

    cache
        .borrow_mut()
        .order_book_mut(&strategy.config.instrument_id)
        .unwrap()
        .add(extra, 0, 3, UnixNanos::default());
    strategy.on_book_deltas(&deltas).unwrap();
    let messages = commands.get_messages();
    assert_eq!(messages.len(), 1);

    let TradingCommand::SubmitOrder(command) = &messages[0] else {
        panic!("expected submit order");
    };

    assert_eq!(command.order_init.order_side, OrderSide::Sell);
    assert_eq!(command.order_init.quantity, Quantity::from("0.123456"));
    assert_eq!(command.order_init.price, Some(Price::from("100.00")));
}

#[rstest]
#[case("size")]
#[case("minimum")]
#[case("ratio_zero")]
#[case("ratio_one")]
#[case("seconds_negative")]
#[case("seconds_nan")]
#[case("seconds_infinite")]
#[case("seconds_overflow")]
#[case("managed_stop")]
#[case("gtd_expiry")]
#[case("contingent_orders")]
fn test_invalid_config(#[case] field: &str) {
    let mut config = config();
    match field {
        "size" => config.max_trade_size = Quantity::from("0"),
        "minimum" => config.trigger_min_size = dec!(-1),
        "ratio_zero" => config.trigger_imbalance_ratio = Decimal::ZERO,
        "ratio_one" => config.trigger_imbalance_ratio = Decimal::ONE,
        "seconds_negative" => config.min_seconds_between_triggers = -1.0,
        "seconds_nan" => config.min_seconds_between_triggers = f64::NAN,
        "seconds_infinite" => config.min_seconds_between_triggers = f64::INFINITY,
        "seconds_overflow" => config.min_seconds_between_triggers = u64::MAX as f64,
        "managed_stop" => {
            config.dry_run = true;
            config.base.manage_stop = true;
        }
        "gtd_expiry" => {
            config.dry_run = true;
            config.base.manage_gtd_expiry = true;
        }
        "contingent_orders" => {
            config.dry_run = true;
            config.base.manage_contingent_orders = true;
        }
        _ => unreachable!(),
    }

    assert!(OrderBookImbalance::from_config(config).is_err());
}

#[rstest]
fn test_invalid_base_config_returns_error() {
    let mut config = config();
    config.base.order_id_tag = Some("runner-1".to_string());
    let error = OrderBookImbalance::from_config(config).unwrap_err();
    assert_eq!(
        error.to_string(),
        "`order_id_tag` cannot contain the '-' strategy ID separator, was 'runner-1'"
    );
}

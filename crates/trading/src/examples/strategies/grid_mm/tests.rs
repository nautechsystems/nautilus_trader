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
    clock::{Clock, VirtualClock},
    messages::execution::TradingCommand,
    msgbus::{self, MessagingSwitchboard, stubs::get_typed_into_message_saving_handler},
};
use nautilus_model::{
    data::QuoteTick,
    enums::{OrderSide, OrderStatus, OrderType},
    events::{
        OrderCanceled, OrderEventAny, OrderExpired, OrderRejected,
        order::spec::{
            OrderCanceledSpec, OrderExpiredSpec, OrderFillVoidedSpec, OrderRejectedSpec,
        },
    },
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, StrategyId, TradeId, TraderId, VenueOrderId,
    },
    instruments::{InstrumentAny, stubs::crypto_perpetual_ethusdt},
    orders::{
        Order, OrderTestBuilder,
        stubs::{OrderFilledTestBuilder, TestOrderEventStubs},
    },
    types::{Price, Quantity},
};
use nautilus_portfolio::portfolio::Portfolio;
use rstest::rstest;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use ustr::Ustr;

use super::{GridMarketMaker, GridMarketMakerConfig};
use crate::strategy::Strategy;

const PRECISION: u8 = 2;

fn create_strategy(
    num_levels: usize,
    grid_step_bps: u32,
    skew_factor: f64,
    max_position: Quantity,
    requote_threshold_bps: u32,
) -> GridMarketMaker {
    let config = GridMarketMakerConfig::builder()
        .instrument_id(InstrumentId::from("ETHUSDT-PERP.BINANCE"))
        .max_position(max_position)
        .trade_size(Quantity::from("0.100"))
        .num_levels(num_levels)
        .grid_step_bps(grid_step_bps)
        .skew_factor(skew_factor)
        .requote_threshold_bps(requote_threshold_bps)
        .build();

    let mut strategy = GridMarketMaker::new(config);
    strategy.instrument = Some(InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt()));
    strategy
}

fn mid(value: &str) -> Price {
    Price::new(value.parse::<f64>().unwrap(), PRECISION)
}

fn quote(bid: &str, ask: &str) -> QuoteTick {
    QuoteTick::new(
        InstrumentId::from("ETHUSDT-PERP.BINANCE"),
        Price::from(bid),
        Price::from(ask),
        Quantity::from(1),
        Quantity::from(1),
        0.into(),
        0.into(),
    )
}

#[rstest]
fn test_config_defaults() {
    let config = GridMarketMakerConfig::builder()
        .instrument_id(InstrumentId::from("ETHUSDT-PERP.BINANCE"))
        .max_position(Quantity::from("10.0"))
        .build();

    assert_eq!(
        config.base.strategy_id,
        Some(StrategyId::from("GRID_MM-001"))
    );
    assert_eq!(config.base.order_id_tag, Some("001".to_string()));
    assert_eq!(config.trade_size, None);
    assert_eq!(config.num_levels, 3);
    assert_eq!(config.grid_step_bps, 10);
    assert_eq!(config.skew_factor, 0.0);
    assert_eq!(config.requote_threshold_bps, 5);
    assert_eq!(config.expire_time_secs, None);
    assert!(!config.on_cancel_resubmit);
}

#[rstest]
fn test_should_requote_true_when_no_previous_quote() {
    let strategy = create_strategy(3, 100, 0.0, Quantity::from("10.0"), 5);
    assert!(strategy.should_requote(dec!(1000.00)));
}

#[rstest]
fn test_should_requote_false_within_threshold() {
    let mut strategy = create_strategy(3, 100, 0.0, Quantity::from("10.0"), 5);
    strategy.last_quoted_mid = Some(dec!(1000.00));
    assert!(!strategy.should_requote(dec!(1000.30)));
}

#[rstest]
fn test_should_requote_true_at_threshold() {
    let mut strategy = create_strategy(3, 100, 0.0, Quantity::from("10.0"), 5);
    strategy.last_quoted_mid = Some(dec!(1000.00));
    assert!(strategy.should_requote(dec!(1000.50)));
}

#[rstest]
fn test_should_requote_true_beyond_threshold_negative() {
    let mut strategy = create_strategy(3, 100, 0.0, Quantity::from("10.0"), 5);
    strategy.last_quoted_mid = Some(dec!(1000.00));
    assert!(strategy.should_requote(dec!(999.40)));
}

#[rstest]
fn test_grid_orders_flat_position_symmetric() {
    // 1% geometric grid: buy = mid × 0.99^level, sell = mid × 1.01^level
    let strategy = create_strategy(3, 100, 0.0, Quantity::from("10.0"), 5);
    let orders = strategy
        .grid_orders(dec!(1000.00), dec!(0.0), dec!(0), dec!(0))
        .unwrap();

    assert_eq!(orders.len(), 6);

    let buys: Vec<_> = orders
        .iter()
        .filter(|(s, _)| *s == OrderSide::Buy)
        .collect();
    let sells: Vec<_> = orders
        .iter()
        .filter(|(s, _)| *s == OrderSide::Sell)
        .collect();
    assert_eq!(buys.len(), 3);
    assert_eq!(sells.len(), 3);

    // Buy prices floor to tick: 0.99^1=990.00, 0.99^2=980.10, 0.99^3≈970.299→970.29
    assert_eq!(buys[0].1, mid("990.00"));
    assert_eq!(buys[1].1, mid("980.10"));
    assert_eq!(buys[2].1, mid("970.29"));

    // Sell prices ceil to tick: 1.01^1=1010.00, 1.01^2=1020.10, 1.01^3≈1030.301→1030.31
    assert_eq!(sells[0].1, mid("1010.00"));
    assert_eq!(sells[1].1, mid("1020.10"));
    assert_eq!(sells[2].1, mid("1030.31"));
}

#[rstest]
fn test_grid_orders_skew_shifts_prices() {
    // 500 bps (5%) geometric grid, skew_factor=1.0, net_position=2.0 → skew_f64=2.0
    let strategy = create_strategy(1, 500, 1.0, Quantity::from("10.0"), 5);
    let orders = strategy
        .grid_orders(dec!(1000.00), dec!(2.0), dec!(2), dec!(2))
        .unwrap();

    assert_eq!(orders.len(), 2);
    // Buy: 1000 × 0.95^1 - 2.0 = 950.0 - 2.0 = 948.0
    assert_eq!(orders[0], (OrderSide::Buy, mid("948.00")));
    // Sell: 1000 × 1.05^1 - 2.0 = 1050.0 - 2.0 = 1048.0
    assert_eq!(orders[1], (OrderSide::Sell, mid("1048.00")));
}

fn count_side(orders: &[(OrderSide, Price)], side: OrderSide) -> usize {
    orders.iter().filter(|(s, _)| *s == side).count()
}

#[rstest]
fn test_grid_orders_max_position_limits_buy_levels() {
    // net_position=9.9, trade_size=0.1, max=10.0 → only 1 buy level fits
    let strategy = create_strategy(3, 100, 0.0, Quantity::from("10.0"), 5);
    let orders = strategy
        .grid_orders(dec!(1000.00), dec!(9.9), dec!(9.9), dec!(9.9))
        .unwrap();

    assert_eq!(count_side(&orders, OrderSide::Buy), 1);
    assert_eq!(count_side(&orders, OrderSide::Sell), 3);
}

#[rstest]
fn test_grid_orders_max_position_limits_sell_levels() {
    // net_position=-9.9, trade_size=0.1, max=10.0 → only 1 sell level fits
    let strategy = create_strategy(3, 100, 0.0, Quantity::from("10.0"), 5);
    let orders = strategy
        .grid_orders(dec!(1000.00), dec!(-9.9), dec!(-9.9), dec!(-9.9))
        .unwrap();

    assert_eq!(count_side(&orders, OrderSide::Buy), 3);
    assert_eq!(count_side(&orders, OrderSide::Sell), 1);
}

#[rstest]
fn test_grid_orders_max_position_blocks_all_buys() {
    // net_position=10.0 (at max) → no buys, all sells
    let strategy = create_strategy(3, 100, 0.0, Quantity::from("10.0"), 5);
    let orders = strategy
        .grid_orders(dec!(1000.00), dec!(10.0), dec!(10), dec!(10))
        .unwrap();

    assert_eq!(count_side(&orders, OrderSide::Buy), 0);
    assert_eq!(count_side(&orders, OrderSide::Sell), 3);
}

#[rstest]
fn test_grid_orders_projected_exposure_across_levels() {
    // max_position=0.15, trade_size=0.1, 3 levels → only 1 level fits per side
    let strategy = create_strategy(3, 100, 0.0, Quantity::from("0.150"), 5);
    let orders = strategy
        .grid_orders(dec!(1000.00), dec!(0.0), dec!(0), dec!(0))
        .unwrap();

    assert_eq!(count_side(&orders, OrderSide::Buy), 1);
    assert_eq!(count_side(&orders, OrderSide::Sell), 1);
}

#[rstest]
fn test_grid_orders_empty_when_fully_constrained() {
    // max_position=0.05, trade_size=0.1 → nothing fits
    let strategy = create_strategy(3, 100, 0.0, Quantity::from("0.050"), 5);
    let orders = strategy
        .grid_orders(dec!(1000.00), dec!(0.0), dec!(0), dec!(0))
        .unwrap();
    assert!(orders.is_empty());
}

#[rstest]
fn test_grid_orders_errors_when_instrument_not_resolved() {
    let config = GridMarketMakerConfig::builder()
        .instrument_id(InstrumentId::from("ETHUSDT-PERP.BINANCE"))
        .max_position(Quantity::from("10.0"))
        .trade_size(Quantity::from("0.100"))
        .build();
    let strategy = GridMarketMaker::new(config);

    let err = strategy
        .grid_orders(dec!(1000.00), dec!(0.0), dec!(0), dec!(0))
        .unwrap_err()
        .to_string();

    assert_eq!(
        err,
        "Cannot compute grid orders: instrument is not resolved"
    );
}

#[rstest]
fn test_grid_orders_reports_skewed_price_overflow() {
    let strategy = create_strategy(1, 50, 1e16, Quantity::from("10.0"), 5);
    let position = dec!(-7922816251426.433759354395);

    let e = strategy
        .grid_orders(dec!(1000), position, position, position)
        .unwrap_err();

    assert_eq!(
        e.to_string(),
        "Skewed grid buy price exceeds the decimal range"
    );
}

#[rstest]
fn test_on_quote_errors_when_instrument_not_resolved() {
    let config = GridMarketMakerConfig::builder()
        .instrument_id(InstrumentId::from("ETHUSDT-PERP.BINANCE"))
        .max_position(Quantity::from("10.0"))
        .trade_size(Quantity::from("0.100"))
        .build();
    let mut strategy = GridMarketMaker::new(config);

    let e = strategy.on_quote(&quote("100.00", "101.00")).unwrap_err();

    assert_eq!(
        e.to_string(),
        "Cannot handle quote: instrument is not resolved"
    );
    assert_eq!(strategy.last_quoted_mid, None);
}

#[rstest]
#[case("100", "101", dec!(100.5))]
#[cfg_attr(feature = "defi", case("100.000000000000000001", "101.000000000000000001", dec!(100.500000000000000001)))]
fn test_on_quote_keeps_midpoint_until_grid_rounding(
    #[case] bid: &str,
    #[case] ask: &str,
    #[case] midpoint: Decimal,
) {
    let mut strategy = create_strategy(1, 50, 0.0, Quantity::from("10.0"), 5);
    let mut instrument = crypto_perpetual_ethusdt();
    instrument.price_precision = 0;
    instrument.price_increment = Price::from("1");
    let instrument = InstrumentAny::CryptoPerpetual(instrument);
    strategy.instrument = Some(instrument.clone());
    let cache = register_with_cache(&mut strategy);
    cache.borrow_mut().add_instrument(instrument).unwrap();
    let (handler, commands) = get_typed_into_message_saving_handler::<TradingCommand>(None);
    msgbus::register_trading_command_endpoint(
        MessagingSwitchboard::risk_engine_queue_execute(),
        handler,
    );

    strategy.on_quote(&quote(bid, ask)).unwrap();

    assert_eq!(strategy.last_quoted_mid, Some(midpoint));
    let cache = cache.borrow();

    let prices: Vec<_> = commands
        .get_messages()
        .iter()
        .map(|command| {
            let TradingCommand::SubmitOrder(command) = command else {
                panic!("Expected SubmitOrder, received {command:?}");
            };

            let order = cache.order(&command.client_order_id).unwrap();
            (order.order_side(), order.price().unwrap())
        })
        .collect();

    assert_eq!(
        prices,
        [
            (OrderSide::Buy, Price::from("99")),
            (OrderSide::Sell, Price::from("102"))
        ]
    );
}

#[cfg(feature = "defi")]
#[rstest]
fn test_grid_prices_from_native_mid() {
    let strategy = create_strategy(1, 10, 0.0, Quantity::from("10.0"), 5);
    let mid = dec!(1000.000000000000000001);

    let orders = strategy
        .grid_orders(mid, dec!(0), dec!(0), dec!(0))
        .unwrap();

    assert_eq!(
        orders,
        [
            (OrderSide::Buy, Price::from("999.00")),
            (OrderSide::Sell, Price::from("1001.01"))
        ]
    );
}

fn order_canceled(client_order_id: &str) -> OrderCanceled {
    OrderCanceledSpec::builder()
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("GRID_MM-001"))
        .instrument_id(InstrumentId::from("ETHUSDT-PERP.BINANCE"))
        .client_order_id(ClientOrderId::from(client_order_id))
        .build()
}

fn create_cancel_resubmit_strategy() -> GridMarketMaker {
    let config = GridMarketMakerConfig::builder()
        .instrument_id(InstrumentId::from("ETHUSDT-PERP.BINANCE"))
        .max_position(Quantity::from("10.0"))
        .trade_size(Quantity::from("0.100"))
        .on_cancel_resubmit(true)
        .build();

    GridMarketMaker::new(config)
}

#[rstest]
fn test_on_order_canceled_self_cancel_preserves_last_quoted_mid() {
    let mut strategy = create_cancel_resubmit_strategy();
    strategy.last_quoted_mid = Some(dec!(1000.00));
    strategy
        .pending_self_cancels
        .insert(ClientOrderId::from("O-001"));

    let event = order_canceled("O-001");
    strategy.on_order_canceled(&event);

    assert!(strategy.pending_self_cancels.is_empty());
    assert_eq!(strategy.last_quoted_mid, Some(dec!(1000.00)));
}

#[rstest]
fn test_on_order_canceled_protocol_cancel_resets_last_quoted_mid() {
    // ID not in pending set → protocol-initiated cancel resets mid
    let mut strategy = create_cancel_resubmit_strategy();
    strategy.last_quoted_mid = Some(dec!(1000.00));

    let event = order_canceled("O-999");
    strategy.on_order_canceled(&event);

    assert_eq!(strategy.last_quoted_mid, None);
}

#[rstest]
fn test_on_order_canceled_self_cancel_then_protocol_cancel() {
    let mut strategy = create_cancel_resubmit_strategy();
    strategy.last_quoted_mid = Some(dec!(1000.00));
    strategy
        .pending_self_cancels
        .insert(ClientOrderId::from("O-001"));

    // Self-cancel consumed
    let self_event = order_canceled("O-001");
    strategy.on_order_canceled(&self_event);
    assert_eq!(strategy.last_quoted_mid, Some(dec!(1000.00)));

    // Protocol cancel triggers reset
    let protocol_event = order_canceled("O-002");
    strategy.on_order_canceled(&protocol_event);
    assert_eq!(strategy.last_quoted_mid, None);
}

#[rstest]
fn test_on_order_canceled_filled_order_does_not_block_protocol_cancel() {
    // Order O-001 tracked as self-cancel but fills before cancel ack,
    // so O-002 (protocol cancel) must still trigger reset
    let mut strategy = create_cancel_resubmit_strategy();
    strategy.last_quoted_mid = Some(dec!(1000.00));
    strategy
        .pending_self_cancels
        .insert(ClientOrderId::from("O-001"));

    // O-001 filled (no cancel event) → O-002 is a protocol cancel
    let event = order_canceled("O-002");
    strategy.on_order_canceled(&event);

    assert_eq!(strategy.last_quoted_mid, None);
}

#[rstest]
#[case::partial_fill_then_self_cancel("0.050", true)]
#[case::full_fill("0.100", false)]
fn test_on_order_filled_keeps_self_cancel_marker_until_full_fill(
    #[case] fill_qty: &str,
    #[case] canceled: bool,
) {
    let mut strategy = create_cancel_resubmit_strategy();
    let cache = register_with_cache(&mut strategy);
    strategy.last_quoted_mid = Some(dec!(1000.00));
    let order_id = ClientOrderId::from("O-001");
    strategy.pending_self_cancels.insert(order_id);

    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let account_id = AccountId::from("ACC-001");
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(InstrumentId::from("ETHUSDT-PERP.BINANCE"))
        .client_order_id(order_id)
        .side(OrderSide::Buy)
        .price(mid("999.00"))
        .quantity(Quantity::from("0.100"))
        .build();
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    cache
        .borrow_mut()
        .update_order(&TestOrderEventStubs::submitted(&order, account_id))
        .unwrap();
    let order = cache
        .borrow_mut()
        .update_order(&TestOrderEventStubs::accepted(
            &order,
            account_id,
            VenueOrderId::from("V-001"),
        ))
        .unwrap();
    let filled = OrderFilledTestBuilder::new(&order, &instrument)
        .last_qty(Quantity::from(fill_qty))
        .build();
    let order = cache.borrow_mut().update_order(&filled).unwrap();

    let OrderEventAny::Filled(fill) = filled else {
        panic!("Expected OrderFilled");
    };

    // Handle the fill only after the cache holds every event, as queued delivery allows
    if canceled {
        let event = TestOrderEventStubs::canceled(&order, account_id, None);
        cache.borrow_mut().update_order(&event).unwrap();

        let OrderEventAny::Canceled(cancel) = event else {
            panic!("Expected OrderCanceled");
        };

        strategy.on_order_filled(&fill);
        strategy.on_order_canceled(&cancel);
    } else {
        strategy.on_order_filled(&fill);
    }

    assert!(strategy.pending_self_cancels.is_empty());
    assert_eq!(strategy.last_quoted_mid, Some(dec!(1000.00)));
}

#[rstest]
fn test_on_order_filled_clears_self_cancel_marker_when_fill_closes_order_voided() {
    let mut strategy = create_cancel_resubmit_strategy();
    let cache = register_with_cache(&mut strategy);
    strategy.last_quoted_mid = Some(dec!(1000.00));
    let order_id = ClientOrderId::from("O-001");
    strategy.pending_self_cancels.insert(order_id);

    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let account_id = AccountId::from("ACC-001");
    let order = OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(InstrumentId::from("ETHUSDT-PERP.BINANCE"))
        .client_order_id(order_id)
        .side(OrderSide::Buy)
        .price(Price::from("999.00"))
        .quantity(Quantity::from("0.100"))
        .build();
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    cache
        .borrow_mut()
        .update_order(&TestOrderEventStubs::submitted(&order, account_id))
        .unwrap();
    let order = cache
        .borrow_mut()
        .update_order(&TestOrderEventStubs::accepted(
            &order,
            account_id,
            VenueOrderId::from("V-001"),
        ))
        .unwrap();
    let first = OrderFilledTestBuilder::new(&order, &instrument)
        .trade_id(TradeId::from("T-001"))
        .last_qty(Quantity::from("0.050"))
        .build();
    cache.borrow_mut().update_order(&first).unwrap();

    let OrderEventAny::Filled(first) = first else {
        panic!("Expected OrderFilled");
    };

    let void = OrderFillVoidedSpec::builder()
        .trader_id(first.trader_id)
        .strategy_id(first.strategy_id)
        .instrument_id(first.instrument_id)
        .client_order_id(order_id)
        .venue_order_id(first.venue_order_id)
        .account_id(first.account_id)
        .trade_id(first.trade_id)
        .voided_qty(first.last_qty)
        .order_side(first.order_side)
        .order_type(first.order_type)
        .last_px(first.last_px)
        .currency(first.currency)
        .liquidity_side(first.liquidity_side)
        .maybe_position_id(first.position_id)
        .build();
    let order = cache
        .borrow_mut()
        .update_order(&OrderEventAny::FillVoided(void))
        .unwrap();

    // A fill that exhausts the leaves after a non-reopened void closes the order as Voided
    let remainder = OrderFilledTestBuilder::new(&order, &instrument)
        .trade_id(TradeId::from("T-002"))
        .last_qty(Quantity::from("0.050"))
        .build();
    let order = cache.borrow_mut().update_order(&remainder).unwrap();

    let OrderEventAny::Filled(remainder) = remainder else {
        panic!("Expected OrderFilled");
    };

    strategy.on_order_filled(&remainder);

    assert_eq!(order.status(), OrderStatus::Voided);
    assert!(strategy.pending_self_cancels.is_empty());
    assert_eq!(strategy.last_quoted_mid, Some(dec!(1000.00)));
}

fn register_with_cache(strategy: &mut GridMarketMaker) -> Rc<RefCell<Cache>> {
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache = Rc::new(RefCell::new(Cache::default()));

    let portfolio = Rc::new(RefCell::new(Portfolio::new(
        clock.clone(),
        cache.clone(),
        None,
    )));

    strategy
        .core
        .register(
            TraderId::from("TESTER-001"),
            clock,
            cache.clone(),
            portfolio,
        )
        .unwrap();
    cache
}

#[rstest]
fn test_on_order_canceled_without_resubmit_does_nothing() {
    // on_cancel_resubmit=false: cancel never resets mid
    let mut strategy = create_strategy(3, 100, 0.0, Quantity::from("10.0"), 5);
    strategy.last_quoted_mid = Some(dec!(1000.00));

    let event = order_canceled("O-001");
    strategy.on_order_canceled(&event);

    assert_eq!(strategy.last_quoted_mid, Some(dec!(1000.00)));
}

fn order_rejected(client_order_id: &str) -> OrderRejected {
    OrderRejectedSpec::builder()
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("GRID_MM-001"))
        .instrument_id(InstrumentId::from("ETHUSDT-PERP.BINANCE"))
        .client_order_id(ClientOrderId::from(client_order_id))
        .account_id(AccountId::from("ACC-001"))
        .reason(Ustr::from("POST_ONLY_ORDER"))
        .due_post_only(true)
        .build()
}

fn order_expired(client_order_id: &str) -> OrderExpired {
    OrderExpiredSpec::builder()
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("GRID_MM-001"))
        .instrument_id(InstrumentId::from("ETHUSDT-PERP.BINANCE"))
        .client_order_id(ClientOrderId::from(client_order_id))
        .build()
}

#[rstest]
fn test_on_order_rejected_discards_pending_and_resets_mid() {
    let mut strategy = create_strategy(3, 100, 0.0, Quantity::from("10.0"), 5);
    strategy.last_quoted_mid = Some(dec!(1000.00));
    strategy
        .pending_self_cancels
        .insert(ClientOrderId::from("O-001"));

    strategy.on_order_rejected(order_rejected("O-001"));

    assert!(strategy.pending_self_cancels.is_empty());
    assert_eq!(strategy.last_quoted_mid, None);
}

#[rstest]
fn test_on_order_rejected_unknown_id_still_resets_mid() {
    let mut strategy = create_strategy(3, 100, 0.0, Quantity::from("10.0"), 5);
    strategy.last_quoted_mid = Some(dec!(1000.00));

    strategy.on_order_rejected(order_rejected("O-999"));

    assert_eq!(strategy.last_quoted_mid, None);
}

#[rstest]
fn test_on_order_expired_discards_pending_and_resets_mid() {
    let mut strategy = create_strategy(3, 100, 0.0, Quantity::from("10.0"), 5);
    strategy.last_quoted_mid = Some(dec!(1000.00));
    strategy
        .pending_self_cancels
        .insert(ClientOrderId::from("O-001"));

    strategy.on_order_expired(order_expired("O-001"));

    assert!(strategy.pending_self_cancels.is_empty());
    assert_eq!(strategy.last_quoted_mid, None);
}

#[rstest]
fn test_on_order_expired_unknown_id_still_resets_mid() {
    let mut strategy = create_strategy(3, 100, 0.0, Quantity::from("10.0"), 5);
    strategy.last_quoted_mid = Some(dec!(1000.00));

    strategy.on_order_expired(order_expired("O-999"));

    assert_eq!(strategy.last_quoted_mid, None);
}

#[rstest]
fn test_on_reset_clears_all_state() {
    let mut strategy = create_strategy(3, 100, 0.0, Quantity::from("10.0"), 5);
    strategy.last_quoted_mid = Some(dec!(1000.00));
    strategy
        .pending_self_cancels
        .insert(ClientOrderId::from("O-001"));

    strategy.on_reset().unwrap();

    assert!(strategy.instrument.is_none());
    assert_eq!(strategy.last_quoted_mid, None);
    assert!(strategy.pending_self_cancels.is_empty());
    // trade_size reverts to the configured value
    assert_eq!(strategy.trade_size, Some(Quantity::from("0.100")));
}

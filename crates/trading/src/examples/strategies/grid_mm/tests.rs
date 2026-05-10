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

use nautilus_common::actor::DataActor;
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    enums::OrderSide,
    events::{OrderCanceled, OrderExpired, OrderRejected},
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId},
    instruments::{InstrumentAny, stubs::crypto_perpetual_ethusdt},
    types::{Price, Quantity},
};
use rstest::rstest;
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
    let config =
        GridMarketMakerConfig::new(InstrumentId::from("ETHUSDT-PERP.BINANCE"), max_position)
            .with_trade_size(Quantity::from("0.100"))
            .with_num_levels(num_levels)
            .with_grid_step_bps(grid_step_bps)
            .with_skew_factor(skew_factor)
            .with_requote_threshold_bps(requote_threshold_bps);

    let mut strategy = GridMarketMaker::new(config);
    strategy.price_precision = Some(PRECISION);
    strategy.instrument = Some(InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt()));
    strategy
}

fn mid(value: &str) -> Price {
    Price::new(value.parse::<f64>().unwrap(), PRECISION)
}

#[rstest]
fn test_should_requote_true_when_no_previous_quote() {
    let strategy = create_strategy(3, 100, 0.0, Quantity::from("10.0"), 5);
    assert!(strategy.should_requote(mid("1000.00")));
}

#[rstest]
fn test_should_requote_false_within_threshold() {
    let mut strategy = create_strategy(3, 100, 0.0, Quantity::from("10.0"), 5);
    strategy.last_quoted_mid = Some(mid("1000.00"));
    assert!(!strategy.should_requote(mid("1000.30")));
}

#[rstest]
fn test_should_requote_true_at_threshold() {
    let mut strategy = create_strategy(3, 100, 0.0, Quantity::from("10.0"), 5);
    strategy.last_quoted_mid = Some(mid("1000.00"));
    assert!(strategy.should_requote(mid("1000.50")));
}

#[rstest]
fn test_should_requote_true_beyond_threshold_negative() {
    let mut strategy = create_strategy(3, 100, 0.0, Quantity::from("10.0"), 5);
    strategy.last_quoted_mid = Some(mid("1000.00"));
    assert!(strategy.should_requote(mid("999.40")));
}

#[rstest]
fn test_grid_orders_flat_position_symmetric() {
    // 1% geometric grid: buy = mid × 0.99^level, sell = mid × 1.01^level
    let strategy = create_strategy(3, 100, 0.0, Quantity::from("10.0"), 5);
    let orders = strategy.grid_orders(mid("1000.00"), 0.0, dec!(0), dec!(0));

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
    let orders = strategy.grid_orders(mid("1000.00"), 2.0, dec!(2), dec!(2));

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
    let orders = strategy.grid_orders(mid("1000.00"), 9.9, dec!(9.9), dec!(9.9));

    assert_eq!(count_side(&orders, OrderSide::Buy), 1);
    assert_eq!(count_side(&orders, OrderSide::Sell), 3);
}

#[rstest]
fn test_grid_orders_max_position_limits_sell_levels() {
    // net_position=-9.9, trade_size=0.1, max=10.0 → only 1 sell level fits
    let strategy = create_strategy(3, 100, 0.0, Quantity::from("10.0"), 5);
    let orders = strategy.grid_orders(mid("1000.00"), -9.9, dec!(-9.9), dec!(-9.9));

    assert_eq!(count_side(&orders, OrderSide::Buy), 3);
    assert_eq!(count_side(&orders, OrderSide::Sell), 1);
}

#[rstest]
fn test_grid_orders_max_position_blocks_all_buys() {
    // net_position=10.0 (at max) → no buys, all sells
    let strategy = create_strategy(3, 100, 0.0, Quantity::from("10.0"), 5);
    let orders = strategy.grid_orders(mid("1000.00"), 10.0, dec!(10), dec!(10));

    assert_eq!(count_side(&orders, OrderSide::Buy), 0);
    assert_eq!(count_side(&orders, OrderSide::Sell), 3);
}

#[rstest]
fn test_grid_orders_projected_exposure_across_levels() {
    // max_position=0.15, trade_size=0.1, 3 levels → only 1 level fits per side
    let strategy = create_strategy(3, 100, 0.0, Quantity::from("0.150"), 5);
    let orders = strategy.grid_orders(mid("1000.00"), 0.0, dec!(0), dec!(0));

    assert_eq!(count_side(&orders, OrderSide::Buy), 1);
    assert_eq!(count_side(&orders, OrderSide::Sell), 1);
}

#[rstest]
fn test_grid_orders_empty_when_fully_constrained() {
    // max_position=0.05, trade_size=0.1 → nothing fits
    let strategy = create_strategy(3, 100, 0.0, Quantity::from("0.050"), 5);
    let orders = strategy.grid_orders(mid("1000.00"), 0.0, dec!(0), dec!(0));
    assert!(orders.is_empty());
}

fn order_canceled(client_order_id: &str) -> OrderCanceled {
    OrderCanceled::new(
        TraderId::from("TESTER-001"),
        StrategyId::from("GRID_MM-001"),
        InstrumentId::from("ETHUSDT-PERP.BINANCE"),
        ClientOrderId::from(client_order_id),
        UUID4::new(),
        0.into(),
        0.into(),
        false,
        None,
        None,
    )
}

fn create_cancel_resubmit_strategy() -> GridMarketMaker {
    let config = GridMarketMakerConfig::new(
        InstrumentId::from("ETHUSDT-PERP.BINANCE"),
        Quantity::from("10.0"),
    )
    .with_trade_size(Quantity::from("0.100"))
    .with_on_cancel_resubmit(true);

    let mut strategy = GridMarketMaker::new(config);
    strategy.price_precision = Some(PRECISION);
    strategy
}

#[rstest]
fn test_on_order_canceled_self_cancel_preserves_last_quoted_mid() {
    let mut strategy = create_cancel_resubmit_strategy();
    strategy.last_quoted_mid = Some(mid("1000.00"));
    strategy
        .pending_self_cancels
        .insert(ClientOrderId::from("O-001"));

    let event = order_canceled("O-001");
    strategy.on_order_canceled(&event).unwrap();

    assert!(strategy.pending_self_cancels.is_empty());
    assert_eq!(strategy.last_quoted_mid, Some(mid("1000.00")));
}

#[rstest]
fn test_on_order_canceled_protocol_cancel_resets_last_quoted_mid() {
    // ID not in pending set → protocol-initiated cancel resets mid
    let mut strategy = create_cancel_resubmit_strategy();
    strategy.last_quoted_mid = Some(mid("1000.00"));

    let event = order_canceled("O-999");
    strategy.on_order_canceled(&event).unwrap();

    assert_eq!(strategy.last_quoted_mid, None);
}

#[rstest]
fn test_on_order_canceled_self_cancel_then_protocol_cancel() {
    let mut strategy = create_cancel_resubmit_strategy();
    strategy.last_quoted_mid = Some(mid("1000.00"));
    strategy
        .pending_self_cancels
        .insert(ClientOrderId::from("O-001"));

    // Self-cancel consumed
    let self_event = order_canceled("O-001");
    strategy.on_order_canceled(&self_event).unwrap();
    assert_eq!(strategy.last_quoted_mid, Some(mid("1000.00")));

    // Protocol cancel triggers reset
    let protocol_event = order_canceled("O-002");
    strategy.on_order_canceled(&protocol_event).unwrap();
    assert_eq!(strategy.last_quoted_mid, None);
}

#[rstest]
fn test_on_order_canceled_filled_order_does_not_block_protocol_cancel() {
    // Order O-001 tracked as self-cancel but fills before cancel ack,
    // so O-002 (protocol cancel) must still trigger reset
    let mut strategy = create_cancel_resubmit_strategy();
    strategy.last_quoted_mid = Some(mid("1000.00"));
    strategy
        .pending_self_cancels
        .insert(ClientOrderId::from("O-001"));

    // O-001 filled (no cancel event) → O-002 is a protocol cancel
    let event = order_canceled("O-002");
    strategy.on_order_canceled(&event).unwrap();

    assert_eq!(strategy.last_quoted_mid, None);
}

#[rstest]
fn test_on_order_canceled_without_resubmit_does_nothing() {
    // on_cancel_resubmit=false: cancel never resets mid
    let mut strategy = create_strategy(3, 100, 0.0, Quantity::from("10.0"), 5);
    strategy.last_quoted_mid = Some(mid("1000.00"));

    let event = order_canceled("O-001");
    strategy.on_order_canceled(&event).unwrap();

    assert_eq!(strategy.last_quoted_mid, Some(mid("1000.00")));
}

fn order_rejected(client_order_id: &str) -> OrderRejected {
    OrderRejected::new(
        TraderId::from("TESTER-001"),
        StrategyId::from("GRID_MM-001"),
        InstrumentId::from("ETHUSDT-PERP.BINANCE"),
        ClientOrderId::from(client_order_id),
        AccountId::from("ACC-001"),
        Ustr::from("POST_ONLY_ORDER"),
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
        true,
    )
}

fn order_expired(client_order_id: &str) -> OrderExpired {
    OrderExpired::new(
        TraderId::from("TESTER-001"),
        StrategyId::from("GRID_MM-001"),
        InstrumentId::from("ETHUSDT-PERP.BINANCE"),
        ClientOrderId::from(client_order_id),
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
        None,
        None,
    )
}

#[rstest]
fn test_on_order_rejected_discards_pending_and_resets_mid() {
    let mut strategy = create_strategy(3, 100, 0.0, Quantity::from("10.0"), 5);
    strategy.last_quoted_mid = Some(mid("1000.00"));
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
    strategy.last_quoted_mid = Some(mid("1000.00"));

    strategy.on_order_rejected(order_rejected("O-999"));

    assert_eq!(strategy.last_quoted_mid, None);
}

#[rstest]
fn test_on_order_expired_discards_pending_and_resets_mid() {
    let mut strategy = create_strategy(3, 100, 0.0, Quantity::from("10.0"), 5);
    strategy.last_quoted_mid = Some(mid("1000.00"));
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
    strategy.last_quoted_mid = Some(mid("1000.00"));

    strategy.on_order_expired(order_expired("O-999"));

    assert_eq!(strategy.last_quoted_mid, None);
}

#[rstest]
fn test_on_reset_clears_all_state() {
    let mut strategy = create_strategy(3, 100, 0.0, Quantity::from("10.0"), 5);
    strategy.last_quoted_mid = Some(mid("1000.00"));
    strategy
        .pending_self_cancels
        .insert(ClientOrderId::from("O-001"));

    strategy.on_reset().unwrap();

    assert!(strategy.instrument.is_none());
    assert!(strategy.price_precision.is_none());
    assert_eq!(strategy.last_quoted_mid, None);
    assert!(strategy.pending_self_cancels.is_empty());
    // trade_size reverts to the configured value
    assert_eq!(strategy.trade_size, Some(Quantity::from("0.100")));
}

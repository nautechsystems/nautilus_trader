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
    data::QuoteTick,
    enums::OrderSide,
    events::{OrderCanceled, OrderExpired, OrderRejected},
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId},
    instruments::{InstrumentAny, stubs::crypto_perpetual_ethusdt},
    types::{Price, Quantity},
};
use rstest::rstest;
use rust_decimal_macros::dec;
use ustr::Ustr;

use super::{CompositeMarketMaker, CompositeMarketMakerConfig};
use crate::strategy::Strategy;

const PRECISION: u8 = 2;

fn instrument_id() -> InstrumentId {
    InstrumentId::from("ETHUSDT-PERP.BINANCE")
}

fn signal_id() -> InstrumentId {
    InstrumentId::from("SEMI-COMPOSITE.SYNTH")
}

fn create_strategy(
    half_spread_bps: u32,
    inventory_skew_factor: f64,
    signal_skew_factor: f64,
    max_position: Quantity,
    requote_threshold_bps: u32,
) -> CompositeMarketMaker {
    let config = CompositeMarketMakerConfig::new(instrument_id(), signal_id(), max_position)
        .with_trade_size(Quantity::from("0.100"))
        .with_half_spread_bps(half_spread_bps)
        .with_inventory_skew_factor(inventory_skew_factor)
        .with_signal_skew_factor(signal_skew_factor)
        .with_requote_threshold_bps(requote_threshold_bps);

    let mut strategy = CompositeMarketMaker::new(config);
    strategy.price_precision = Some(PRECISION);
    strategy.instrument = Some(InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt()));
    strategy
}

fn price(value: &str) -> Price {
    Price::new(value.parse::<f64>().unwrap(), PRECISION)
}

fn quote(instrument: InstrumentId, bid: &str, ask: &str) -> QuoteTick {
    QuoteTick::new(
        instrument,
        price(bid),
        price(ask),
        Quantity::from("1.0"),
        Quantity::from("1.0"),
        UnixNanos::default(),
        UnixNanos::default(),
    )
}

#[rstest]
fn test_should_requote_true_when_no_previous_quote() {
    let strategy = create_strategy(5, 0.0, 0.0, Quantity::from("10.0"), 5);
    assert!(strategy.should_requote(price("1000.00"), 0.0));
}

#[rstest]
fn test_should_requote_false_within_threshold() {
    let mut strategy = create_strategy(5, 0.0, 0.0, Quantity::from("10.0"), 5);
    strategy.last_quoted_anchor = Some(price("1000.00"));
    strategy.last_quoted_residual = Some(0.0);
    assert!(!strategy.should_requote(price("1000.30"), 0.0));
}

#[rstest]
fn test_should_requote_true_at_threshold() {
    let mut strategy = create_strategy(5, 0.0, 0.0, Quantity::from("10.0"), 5);
    strategy.last_quoted_anchor = Some(price("1000.00"));
    strategy.last_quoted_residual = Some(0.0);
    assert!(strategy.should_requote(price("1000.50"), 0.0));
}

#[rstest]
fn test_should_requote_on_residual_change_when_anchor_static() {
    // requote_threshold_bps=5 -> 0.5 price units on 1000 anchor.
    // signal_skew_factor=10.0, residual_delta=0.06 -> price impact 0.6 units.
    let mut strategy = create_strategy(5, 0.0, 10.0, Quantity::from("10.0"), 5);
    strategy.last_quoted_anchor = Some(price("1000.00"));
    strategy.last_quoted_residual = Some(0.0);

    // Anchor unchanged but residual moved enough to clear the threshold.
    assert!(strategy.should_requote(price("1000.00"), 0.06));
}

#[rstest]
fn test_should_not_requote_when_residual_change_below_threshold() {
    // 5 bps threshold on 1000 = 0.5 units. signal_skew=10, residual_delta=0.04 -> 0.4 units.
    let mut strategy = create_strategy(5, 0.0, 10.0, Quantity::from("10.0"), 5);
    strategy.last_quoted_anchor = Some(price("1000.00"));
    strategy.last_quoted_residual = Some(0.0);

    assert!(!strategy.should_requote(price("1000.00"), 0.04));
}

#[rstest]
fn test_residual_gate_inactive_when_signal_skew_zero() {
    // With signal_skew_factor=0 the residual cannot shift quotes, so changes
    // in residual must not trigger a requote on their own.
    let mut strategy = create_strategy(5, 0.0, 0.0, Quantity::from("10.0"), 5);
    strategy.last_quoted_anchor = Some(price("1000.00"));
    strategy.last_quoted_residual = Some(0.0);

    assert!(!strategy.should_requote(price("1000.00"), 1.0));
}

#[rstest]
fn test_should_requote_true_with_signal_skew_when_no_previous_quote() {
    // First quote: no last anchor and no last residual; even with the residual
    // gate active, should_requote must return true so the strategy seeds quotes.
    let strategy = create_strategy(5, 0.0, 10.0, Quantity::from("10.0"), 5);
    assert!(strategy.should_requote(price("1000.00"), 0.05));
}

#[rstest]
fn test_signal_residual_zero_without_signal() {
    let strategy = create_strategy(5, 0.0, 1.0, Quantity::from("10.0"), 5);
    assert_eq!(strategy.signal_residual(), 0.0);
}

#[rstest]
fn test_signal_residual_normalized_against_baseline() {
    let mut strategy = create_strategy(5, 0.0, 1.0, Quantity::from("10.0"), 5);
    strategy.signal_baseline = Some(100.0);
    strategy.last_signal = Some(110.0);
    assert!((strategy.signal_residual() - 0.10).abs() < 1e-9);
}

#[rstest]
fn test_signal_residual_zero_baseline_guard() {
    // Zero baseline must not divide by zero; the guard returns 0.0.
    let mut strategy = create_strategy(5, 0.0, 1.0, Quantity::from("10.0"), 5);
    strategy.signal_baseline = Some(0.0);
    strategy.last_signal = Some(110.0);
    assert_eq!(strategy.signal_residual(), 0.0);
}

#[rstest]
fn test_should_requote_on_residual_with_zero_anchor() {
    // An anchor of zero short-circuits the residual gate to false to avoid
    // dividing by zero, even with a residual change far above the threshold.
    let mut strategy = create_strategy(5, 0.0, 10.0, Quantity::from("10.0"), 5);
    strategy.last_quoted_anchor = Some(price("1000.00"));
    strategy.last_quoted_residual = Some(0.0);

    assert!(!strategy.should_requote_on_residual(1.0, price("0.00")));
}

#[rstest]
fn test_should_requote_on_anchor_with_zero_last_anchor() {
    // A zero last anchor short-circuits to true so the next quote can place.
    let mut strategy = create_strategy(5, 0.0, 0.0, Quantity::from("10.0"), 5);
    strategy.last_quoted_anchor = Some(price("0.00"));

    assert!(strategy.should_requote_on_anchor(price("1000.00")));
}

#[rstest]
fn test_compute_quotes_flat_no_signal_symmetric() {
    // 10 bps half-spread on 1000 anchor -> 1.00 each side.
    let strategy = create_strategy(10, 0.0, 0.0, Quantity::from("10.0"), 5);
    let quotes = strategy.compute_quotes(price("1000.00"), 0.0, 0.0, dec!(0), dec!(0));

    assert_eq!(quotes.len(), 2);
    assert_eq!(quotes[0], (OrderSide::Buy, price("999.00")));
    assert_eq!(quotes[1], (OrderSide::Sell, price("1001.00")));
}

#[rstest]
fn test_compute_quotes_inventory_skew_long_shifts_down() {
    // 100 bps half-spread, inv_skew=1.0, net_position=2.0 -> total shift -2.0.
    let strategy = create_strategy(100, 1.0, 0.0, Quantity::from("10.0"), 5);
    let quotes = strategy.compute_quotes(price("1000.00"), 0.0, 2.0, dec!(2), dec!(2));

    assert_eq!(quotes.len(), 2);
    // bid = 1000 - 10 - 2 = 988.00, ask = 1000 + 10 - 2 = 1008.00
    assert_eq!(quotes[0], (OrderSide::Buy, price("988.00")));
    assert_eq!(quotes[1], (OrderSide::Sell, price("1008.00")));
}

#[rstest]
fn test_compute_quotes_inventory_skew_short_shifts_up() {
    let strategy = create_strategy(100, 1.0, 0.0, Quantity::from("10.0"), 5);
    let quotes = strategy.compute_quotes(price("1000.00"), 0.0, -2.0, dec!(-2), dec!(-2));

    assert_eq!(quotes.len(), 2);
    // total shift = +2.0, bid = 992.00, ask = 1012.00
    assert_eq!(quotes[0], (OrderSide::Buy, price("992.00")));
    assert_eq!(quotes[1], (OrderSide::Sell, price("1012.00")));
}

#[rstest]
fn test_compute_quotes_signal_skew_positive_residual_lifts() {
    // 100 bps half-spread, signal_skew=10.0, residual=+0.05 -> total shift +0.50.
    let strategy = create_strategy(100, 0.0, 10.0, Quantity::from("10.0"), 5);
    let quotes = strategy.compute_quotes(price("1000.00"), 0.05, 0.0, dec!(0), dec!(0));

    assert_eq!(quotes.len(), 2);
    // bid = 1000 - 10 + 0.5 = 990.50, ask = 1000 + 10 + 0.5 = 1010.50
    assert_eq!(quotes[0], (OrderSide::Buy, price("990.50")));
    assert_eq!(quotes[1], (OrderSide::Sell, price("1010.50")));
}

#[rstest]
fn test_compute_quotes_combined_skew() {
    // half=100bps (10.0), inv_skew=1.0 with pos=2 -> -2.0,
    // sig_skew=10.0 with residual=+0.05 -> +0.5. total shift = +0.5 - 2.0 = -1.5.
    let strategy = create_strategy(100, 1.0, 10.0, Quantity::from("10.0"), 5);
    let quotes = strategy.compute_quotes(price("1000.00"), 0.05, 2.0, dec!(2), dec!(2));

    assert_eq!(quotes.len(), 2);
    assert_eq!(quotes[0], (OrderSide::Buy, price("988.50")));
    assert_eq!(quotes[1], (OrderSide::Sell, price("1008.50")));
}

#[rstest]
fn test_compute_quotes_max_position_blocks_buy() {
    // worst_long=10.0 already at cap, trade_size=0.1 -> buy blocked.
    let strategy = create_strategy(10, 0.0, 0.0, Quantity::from("10.0"), 5);
    let quotes = strategy.compute_quotes(price("1000.00"), 0.0, 10.0, dec!(10), dec!(10));

    assert_eq!(quotes.len(), 1);
    assert_eq!(quotes[0].0, OrderSide::Sell);
}

#[rstest]
fn test_compute_quotes_max_position_blocks_sell() {
    let strategy = create_strategy(10, 0.0, 0.0, Quantity::from("10.0"), 5);
    let quotes = strategy.compute_quotes(price("1000.00"), 0.0, -10.0, dec!(-10), dec!(-10));

    assert_eq!(quotes.len(), 1);
    assert_eq!(quotes[0].0, OrderSide::Buy);
}

#[rstest]
fn test_compute_quotes_skew_preserves_spread() {
    // Inventory and signal skew shift both sides equally, so the quoted
    // spread (ask - bid) is invariant in skew under the symmetric model.
    let strategy = create_strategy(50, 0.5, 5.0, Quantity::from("10.0"), 5);
    let flat = strategy.compute_quotes(price("1000.00"), 0.0, 0.0, dec!(0), dec!(0));
    let skewed = strategy.compute_quotes(price("1000.00"), 0.02, 3.0, dec!(3), dec!(3));

    assert_eq!(flat.len(), 2);
    assert_eq!(skewed.len(), 2);

    let flat_spread = flat[1].1.as_f64() - flat[0].1.as_f64();
    let skewed_spread = skewed[1].1.as_f64() - skewed[0].1.as_f64();
    assert!((flat_spread - skewed_spread).abs() < 1e-9);
}

fn order_canceled(client_order_id: &str) -> OrderCanceled {
    OrderCanceled::new(
        TraderId::from("TESTER-001"),
        StrategyId::from("COMPOSITE_MM-001"),
        instrument_id(),
        ClientOrderId::from(client_order_id),
        UUID4::new(),
        0.into(),
        0.into(),
        false,
        None,
        None,
    )
}

fn create_cancel_resubmit_strategy() -> CompositeMarketMaker {
    let config =
        CompositeMarketMakerConfig::new(instrument_id(), signal_id(), Quantity::from("10.0"))
            .with_trade_size(Quantity::from("0.100"))
            .with_on_cancel_resubmit(true);

    let mut strategy = CompositeMarketMaker::new(config);
    strategy.price_precision = Some(PRECISION);
    strategy
}

#[rstest]
fn test_signal_tick_updates_last_signal_and_captures_baseline_once() {
    // Two signal ticks: baseline must capture the first mid and stay there;
    // last_signal must reflect the latest mid.
    let mut strategy = create_strategy(5, 0.0, 1.0, Quantity::from("10.0"), 5);
    let first = quote(signal_id(), "100.00", "100.00");
    let second = quote(signal_id(), "120.00", "120.00");

    strategy.on_quote(&first).unwrap();
    strategy.on_quote(&second).unwrap();

    assert_eq!(strategy.signal_baseline, Some(100.0));
    assert_eq!(strategy.last_signal, Some(120.0));
}

#[rstest]
fn test_signal_tick_does_not_overwrite_explicit_baseline() {
    // When the config carries an explicit baseline, signal ticks update
    // last_signal but never the baseline.
    let config =
        CompositeMarketMakerConfig::new(instrument_id(), signal_id(), Quantity::from("10.0"))
            .with_trade_size(Quantity::from("0.100"))
            .with_signal_skew_factor(1.0)
            .with_signal_baseline(50.0);
    let mut strategy = CompositeMarketMaker::new(config);
    strategy.price_precision = Some(PRECISION);
    strategy.instrument = Some(InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt()));

    let tick = quote(signal_id(), "120.00", "120.00");
    strategy.on_quote(&tick).unwrap();

    assert_eq!(strategy.signal_baseline, Some(50.0));
    assert_eq!(strategy.last_signal, Some(120.0));
}

#[rstest]
fn test_on_quote_ignores_unrelated_instrument() {
    // A QuoteTick for a third instrument neither updates signal state nor
    // touches the cache (the early-return path covers this).
    let mut strategy = create_strategy(5, 0.0, 1.0, Quantity::from("10.0"), 5);
    let unrelated = InstrumentId::from("XBTUSD.BITMEX");
    let tick = quote(unrelated, "50000.00", "50000.10");

    strategy.on_quote(&tick).unwrap();

    assert_eq!(strategy.last_signal, None);
    assert_eq!(strategy.signal_baseline, None);
    assert_eq!(strategy.last_quoted_anchor, None);
    assert_eq!(strategy.last_quoted_residual, None);
}

#[rstest]
fn test_on_order_canceled_self_cancel_preserves_anchor() {
    let mut strategy = create_cancel_resubmit_strategy();
    strategy.last_quoted_anchor = Some(price("1000.00"));
    strategy.last_quoted_residual = Some(0.05);
    strategy
        .pending_self_cancels
        .insert(ClientOrderId::from("O-001"));

    let event = order_canceled("O-001");
    strategy.on_order_canceled(&event).unwrap();

    assert!(strategy.pending_self_cancels.is_empty());
    assert_eq!(strategy.last_quoted_anchor, Some(price("1000.00")));
    assert_eq!(strategy.last_quoted_residual, Some(0.05));
}

#[rstest]
fn test_on_order_canceled_protocol_cancel_resets_anchor() {
    let mut strategy = create_cancel_resubmit_strategy();
    strategy.last_quoted_anchor = Some(price("1000.00"));
    strategy.last_quoted_residual = Some(0.05);

    let event = order_canceled("O-999");
    strategy.on_order_canceled(&event).unwrap();

    assert_eq!(strategy.last_quoted_anchor, None);
    assert_eq!(strategy.last_quoted_residual, None);
}

#[rstest]
fn test_on_order_canceled_without_resubmit_does_nothing() {
    // on_cancel_resubmit=false: a protocol cancel must not reset state.
    let mut strategy = create_strategy(5, 0.0, 0.0, Quantity::from("10.0"), 5);
    strategy.last_quoted_anchor = Some(price("1000.00"));
    strategy.last_quoted_residual = Some(0.05);

    let event = order_canceled("O-999");
    strategy.on_order_canceled(&event).unwrap();

    assert_eq!(strategy.last_quoted_anchor, Some(price("1000.00")));
    assert_eq!(strategy.last_quoted_residual, Some(0.05));
}

fn order_rejected(client_order_id: &str) -> OrderRejected {
    OrderRejected::new(
        TraderId::from("TESTER-001"),
        StrategyId::from("COMPOSITE_MM-001"),
        instrument_id(),
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
        StrategyId::from("COMPOSITE_MM-001"),
        instrument_id(),
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
fn test_on_order_rejected_discards_pending_and_resets_anchor() {
    let mut strategy = create_strategy(5, 0.0, 0.0, Quantity::from("10.0"), 5);
    strategy.last_quoted_anchor = Some(price("1000.00"));
    strategy.last_quoted_residual = Some(0.05);
    strategy
        .pending_self_cancels
        .insert(ClientOrderId::from("O-001"));

    strategy.on_order_rejected(order_rejected("O-001"));

    assert!(strategy.pending_self_cancels.is_empty());
    assert_eq!(strategy.last_quoted_anchor, None);
    assert_eq!(strategy.last_quoted_residual, None);
}

#[rstest]
fn test_on_order_rejected_unknown_id_still_resets_anchor() {
    // Reject of an id that is not in pending_self_cancels still resets state.
    let mut strategy = create_strategy(5, 0.0, 0.0, Quantity::from("10.0"), 5);
    strategy.last_quoted_anchor = Some(price("1000.00"));
    strategy.last_quoted_residual = Some(0.05);

    strategy.on_order_rejected(order_rejected("O-999"));

    assert_eq!(strategy.last_quoted_anchor, None);
    assert_eq!(strategy.last_quoted_residual, None);
}

#[rstest]
fn test_on_order_expired_discards_pending_and_resets_anchor() {
    let mut strategy = create_strategy(5, 0.0, 0.0, Quantity::from("10.0"), 5);
    strategy.last_quoted_anchor = Some(price("1000.00"));
    strategy.last_quoted_residual = Some(0.05);
    strategy
        .pending_self_cancels
        .insert(ClientOrderId::from("O-001"));

    strategy.on_order_expired(order_expired("O-001"));

    assert!(strategy.pending_self_cancels.is_empty());
    assert_eq!(strategy.last_quoted_anchor, None);
    assert_eq!(strategy.last_quoted_residual, None);
}

#[rstest]
fn test_on_order_expired_unknown_id_still_resets_anchor() {
    let mut strategy = create_strategy(5, 0.0, 0.0, Quantity::from("10.0"), 5);
    strategy.last_quoted_anchor = Some(price("1000.00"));
    strategy.last_quoted_residual = Some(0.05);

    strategy.on_order_expired(order_expired("O-999"));

    assert_eq!(strategy.last_quoted_anchor, None);
    assert_eq!(strategy.last_quoted_residual, None);
}

#[rstest]
fn test_on_reset_clears_all_state() {
    let mut strategy = create_strategy(5, 0.0, 0.0, Quantity::from("10.0"), 5);
    strategy.last_quoted_anchor = Some(price("1000.00"));
    strategy.last_quoted_residual = Some(0.05);
    strategy.signal_baseline = Some(100.0);
    strategy.last_signal = Some(110.0);
    strategy
        .pending_self_cancels
        .insert(ClientOrderId::from("O-001"));

    strategy.on_reset().unwrap();

    assert!(strategy.instrument.is_none());
    assert!(strategy.price_precision.is_none());
    assert_eq!(strategy.last_quoted_anchor, None);
    assert_eq!(strategy.last_quoted_residual, None);
    // Reset reverts baseline to the configured value (None when unset)
    assert_eq!(strategy.signal_baseline, None);
    assert_eq!(strategy.last_signal, None);
    assert!(strategy.pending_self_cancels.is_empty());
    assert_eq!(strategy.trade_size, Some(Quantity::from("0.100")));
}

#[rstest]
fn test_on_reset_reverts_signal_baseline_to_config_value() {
    // When the config carries an explicit baseline, reset reverts to that
    // configured value, not None.
    let config =
        CompositeMarketMakerConfig::new(instrument_id(), signal_id(), Quantity::from("10.0"))
            .with_trade_size(Quantity::from("0.100"))
            .with_signal_baseline(50.0);
    let mut strategy = CompositeMarketMaker::new(config);
    strategy.price_precision = Some(PRECISION);
    strategy.instrument = Some(InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt()));
    // Mutate baseline away from the configured value before resetting.
    strategy.signal_baseline = Some(200.0);
    strategy.last_signal = Some(220.0);

    strategy.on_reset().unwrap();

    assert_eq!(strategy.signal_baseline, Some(50.0));
    assert_eq!(strategy.last_signal, None);
}

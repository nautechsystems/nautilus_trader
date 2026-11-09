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
    timer::{TimeEvent, TimeEventCallback},
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    data::{QuoteTick, greeks::OptionGreekValues, option_chain::OptionGreeks},
    enums::{OmsType, OptionKind, OrderSide, OrderType, TimeInForce},
    events::{
        OrderDenied, OrderEventAny, OrderExpired, OrderFilled, OrderRejected,
        order::spec::{
            OrderDeniedSpec, OrderExpiredSpec, OrderFillVoidedSpec, OrderFilledSpec,
            OrderRejectedSpec,
        },
    },
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, PositionId, StrategyId, Symbol, TradeId,
        TraderId,
    },
    instruments::{CryptoPerpetual, Instrument, InstrumentAny, stubs::crypto_option_btc_deribit},
    orders::{Order, OrderAny, builder::OrderTestBuilder},
    position::Position,
    types::{Currency, Price, Quantity},
};
use nautilus_portfolio::portfolio::Portfolio;
use rstest::rstest;
use ustr::Ustr;

use super::{DeltaNeutralVol, DeltaNeutralVolConfig};
use crate::strategy::Strategy;

fn create_config() -> DeltaNeutralVolConfig {
    DeltaNeutralVolConfig::builder()
        .option_family("BTC-USD".to_string())
        .hedge_instrument_id(InstrumentId::from("BTC-USD-SWAP.OKX"))
        .client_id(ClientId::new("OKX"))
        .build()
}

fn create_strategy() -> DeltaNeutralVol {
    DeltaNeutralVol::new(create_config())
}

fn create_selected_strategy() -> DeltaNeutralVol {
    let mut s = create_strategy();
    s.call_instrument_id = Some(InstrumentId::from("BTC-USD-260327-75000-C.OKX"));
    s.put_instrument_id = Some(InstrumentId::from("BTC-USD-260327-65000-P.OKX"));
    s
}

fn create_initialized_strategy() -> DeltaNeutralVol {
    let mut s = create_selected_strategy();
    s.call_delta = 0.20;
    s.put_delta = -0.20;
    s.call_delta_ready = true;
    s.put_delta_ready = true;
    s
}

fn create_entry_ready_strategy() -> DeltaNeutralVol {
    let mut s = create_initialized_strategy();
    s.call_mark_iv = Some(0.55);
    s.put_mark_iv = Some(0.50);
    s
}

fn quote_tick(instrument_id: InstrumentId, bid: &str, ask: &str) -> QuoteTick {
    QuoteTick::new(
        instrument_id,
        Price::from(bid),
        Price::from(ask),
        Quantity::from("1"),
        Quantity::from("1"),
        UnixNanos::default(),
        UnixNanos::default(),
    )
}

/// The configured hedge instrument, sized in whole contracts (`size_precision` 0), so a
/// fractional delta cannot be expressed as a quantity.
fn hedge_swap_integer_sized() -> CryptoPerpetual {
    CryptoPerpetual::builder()
        .instrument_id(InstrumentId::from("BTC-USD-SWAP.OKX"))
        .raw_symbol(Symbol::from("BTC-USD-SWAP"))
        .base_currency(Currency::BTC())
        .quote_currency(Currency::USD())
        .settlement_currency(Currency::USD())
        .is_inverse(false)
        .price_precision(1)
        .size_precision(0)
        .price_increment(Price::from("0.1"))
        .size_increment(Quantity::from(1))
        .ts_event(0.into())
        .ts_init(0.into())
        .build()
        .unwrap()
}

fn register_strategy(strategy: &mut DeltaNeutralVol) {
    let trader_id = TraderId::from("TESTER-001");
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(VirtualClock::new()));
    let cache = Rc::new(RefCell::new(Cache::default()));
    let portfolio = Rc::new(RefCell::new(Portfolio::new(
        clock.clone(),
        cache.clone(),
        None,
    )));

    strategy
        .core
        .register(trader_id, clock, cache, portfolio)
        .unwrap();
}

fn make_order_rejected(instrument_id: InstrumentId) -> OrderRejected {
    OrderRejectedSpec::builder()
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("DELTA_NEUTRAL_VOL-001"))
        .instrument_id(instrument_id)
        .client_order_id(ClientOrderId::from("O-HEDGE-1"))
        .account_id(AccountId::from("ACC-001"))
        .reason("Test rejection".into())
        .event_id(UUID4::default())
        .build()
}

fn make_order_denied(instrument_id: InstrumentId) -> OrderDenied {
    OrderDeniedSpec::builder()
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("DELTA_NEUTRAL_VOL-001"))
        .instrument_id(instrument_id)
        .client_order_id(ClientOrderId::from("O-HEDGE-1"))
        .reason("Test denial".into())
        .build()
}

fn make_order_expired(instrument_id: InstrumentId) -> OrderExpired {
    OrderExpiredSpec::builder()
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("DELTA_NEUTRAL_VOL-001"))
        .instrument_id(instrument_id)
        .client_order_id(ClientOrderId::from("O-HEDGE-1"))
        .account_id(AccountId::from("ACC-001"))
        .event_id(UUID4::default())
        .build()
}

#[rstest]
fn test_new_sets_strategy_id() {
    let strategy = create_strategy();
    assert_eq!(
        strategy.strategy_id(),
        Some(StrategyId::from("DELTA_NEUTRAL_VOL-001")),
    );
}

#[rstest]
fn test_config_defaults() {
    let config = create_config();
    assert_eq!(config.target_call_delta, 0.20);
    assert_eq!(config.target_put_delta, -0.20);
    assert_eq!(config.contracts, 1);
    assert_eq!(config.rehedge_delta_threshold, 0.5);
    assert_eq!(config.rehedge_interval_secs, 30);
    assert!(config.expiry_filter.is_none());
    assert!(config.entry_premium_offset_ticks.is_none());
}

#[rstest]
fn test_config_builder_methods() {
    let mut config = DeltaNeutralVolConfig::builder()
        .option_family("BTC-USD".to_string())
        .hedge_instrument_id(InstrumentId::from("BTC-USD-SWAP.OKX"))
        .client_id(ClientId::new("OKX"))
        .target_call_delta(0.30)
        .target_put_delta(-0.30)
        .contracts(10)
        .rehedge_delta_threshold(1.0)
        .rehedge_interval_secs(60)
        .expiry_filter("260327".to_string())
        .build();
    config.base.strategy_id = Some(StrategyId::from("CUSTOM-001"));
    config.base.order_id_tag = Some("002".to_string());

    assert_eq!(config.target_call_delta, 0.30);
    assert_eq!(config.target_put_delta, -0.30);
    assert_eq!(config.contracts, 10);
    assert_eq!(config.rehedge_delta_threshold, 1.0);
    assert_eq!(config.rehedge_interval_secs, 60);
    assert_eq!(config.expiry_filter.as_deref(), Some("260327"));
    assert_eq!(
        config.base.strategy_id,
        Some(StrategyId::from("CUSTOM-001"))
    );
    assert_eq!(config.base.order_id_tag.as_deref(), Some("002"));
}

#[rstest]
fn test_portfolio_delta_zero_with_no_positions() {
    let strategy = create_strategy();
    assert_eq!(strategy.portfolio_delta(), 0.0);
}

#[rstest]
fn test_portfolio_delta_from_call_position() {
    let mut strategy = create_strategy();
    strategy.call_delta = 0.25;
    strategy.call_position = -10.0;
    assert!((strategy.portfolio_delta() - (-2.5)).abs() < 1e-10);
}

#[rstest]
fn test_portfolio_delta_from_put_position() {
    let mut strategy = create_strategy();
    strategy.put_delta = -0.25;
    strategy.put_position = -10.0;
    assert!((strategy.portfolio_delta() - 2.5).abs() < 1e-10);
}

#[rstest]
fn test_portfolio_delta_strangle_with_hedge() {
    let mut strategy = create_strategy();

    strategy.call_delta = 0.20;
    strategy.call_position = -5.0;

    strategy.put_delta = -0.20;
    strategy.put_position = -5.0;

    assert!(strategy.portfolio_delta().abs() < 1e-10);
    strategy.hedge_position = 0.5;
    assert!((strategy.portfolio_delta() - 0.5).abs() < 1e-10);
}

#[rstest]
fn test_should_rehedge_true_above_threshold() {
    let mut strategy = create_initialized_strategy();
    strategy.hedge_position = 1.0;
    assert!(strategy.should_rehedge());
}

#[rstest]
fn test_should_rehedge_false_below_threshold() {
    let mut strategy = create_strategy();
    strategy.hedge_position = 0.3;
    assert!(!strategy.should_rehedge());
}

#[rstest]
fn test_should_rehedge_false_at_zero() {
    let strategy = create_strategy();
    assert!(!strategy.should_rehedge());
}

#[rstest]
fn test_should_rehedge_with_custom_threshold() {
    let mut config = create_config();
    config.rehedge_delta_threshold = 0.1;
    let mut strategy = DeltaNeutralVol::new(config);
    strategy.call_instrument_id = Some(InstrumentId::from("BTC-USD-260327-75000-C.OKX"));
    strategy.put_instrument_id = Some(InstrumentId::from("BTC-USD-260327-65000-P.OKX"));
    strategy.call_delta = 0.20;
    strategy.put_delta = -0.20;
    strategy.call_delta_ready = true;
    strategy.put_delta_ready = true;
    strategy.hedge_position = 0.15;
    assert!(strategy.should_rehedge());
}

#[rstest]
fn test_should_rehedge_false_with_only_one_ready_leg() {
    let mut config = create_config();
    config.rehedge_delta_threshold = 0.1;
    let mut strategy = DeltaNeutralVol::new(config);
    strategy.call_instrument_id = Some(InstrumentId::from("BTC-USD-260327-75000-C.OKX"));
    strategy.put_instrument_id = Some(InstrumentId::from("BTC-USD-260327-65000-P.OKX"));
    strategy.call_delta = 0.20;
    strategy.call_delta_ready = true;
    strategy.hedge_position = 0.15;
    assert!(!strategy.should_rehedge());
}

#[rstest]
fn test_hedge_direction_sell_when_portfolio_delta_positive() {
    let mut strategy = create_initialized_strategy();
    strategy.call_delta = 0.30;
    strategy.call_position = -10.0;
    strategy.put_delta = -0.10;
    strategy.put_position = -10.0;

    strategy.hedge_position = 5.0;

    let delta = strategy.portfolio_delta();
    assert!((delta - 3.0).abs() < 1e-10);
    assert!(strategy.should_rehedge());

    let side = if delta > 0.0 {
        OrderSide::Sell
    } else {
        OrderSide::Buy
    };
    assert_eq!(side, OrderSide::Sell);
}

#[rstest]
fn test_hedge_direction_buy_when_portfolio_delta_negative() {
    let mut strategy = create_initialized_strategy();
    strategy.call_delta = 0.30;
    strategy.call_position = -10.0;
    strategy.put_delta = -0.10;
    strategy.put_position = -10.0;

    let delta = strategy.portfolio_delta();
    assert!((delta - (-2.0)).abs() < 1e-10);
    assert!(strategy.should_rehedge());

    let side = if delta > 0.0 {
        OrderSide::Sell
    } else {
        OrderSide::Buy
    };
    assert_eq!(side, OrderSide::Buy);
}

#[rstest]
fn test_position_tracking_hedge_buy_fill() {
    let mut strategy = create_strategy();
    assert_eq!(strategy.hedge_position, 0.0);

    strategy.hedge_position += 1.5;
    assert!((strategy.hedge_position - 1.5).abs() < 1e-10);
    assert!((strategy.portfolio_delta() - 1.5).abs() < 1e-10);
}

#[rstest]
fn test_position_tracking_option_sell_fill() {
    let mut strategy = create_strategy();
    strategy.call_instrument_id = Some(InstrumentId::from("BTC-USD-260327-75000-C.OKX"));
    strategy.call_delta = 0.25;

    strategy.call_position -= 5.0;
    assert!((strategy.call_position - (-5.0)).abs() < 1e-10);
    assert!((strategy.portfolio_delta() - (-1.25)).abs() < 1e-10);
}

#[rstest]
fn test_position_tracking_cumulative_fills() {
    let mut strategy = create_strategy();
    strategy.call_instrument_id = Some(InstrumentId::from("BTC-USD-260327-75000-C.OKX"));
    strategy.put_instrument_id = Some(InstrumentId::from("BTC-USD-260327-65000-P.OKX"));
    strategy.call_delta = 0.20;
    strategy.put_delta = -0.20;
    strategy.call_delta_ready = true;
    strategy.put_delta_ready = true;

    strategy.call_position = -5.0;
    strategy.put_position = -5.0;

    assert!(strategy.portfolio_delta().abs() < 1e-10);
    assert!(!strategy.should_rehedge());

    strategy.call_delta = 0.35;

    assert!((strategy.portfolio_delta() - (-0.75)).abs() < 1e-10);
    assert!(strategy.should_rehedge());

    strategy.hedge_position += 0.75;

    assert!(strategy.portfolio_delta().abs() < 1e-10);
    assert!(!strategy.should_rehedge());
}

#[rstest]
fn test_greeks_initialized_false_when_no_instruments_set() {
    let strategy = create_strategy();
    assert!(!strategy.greeks_initialized());
}

#[rstest]
fn test_greeks_initialized_false_when_only_call_set() {
    let mut strategy = create_strategy();
    strategy.call_instrument_id = Some(InstrumentId::from("BTC-USD-260327-75000-C.OKX"));
    strategy.call_delta = 0.25;
    strategy.call_delta_ready = true;
    assert!(!strategy.greeks_initialized());
}

#[rstest]
fn test_greeks_initialized_false_when_ids_set_but_no_ready_legs() {
    let strategy = create_selected_strategy();
    assert!(!strategy.greeks_initialized());
}

#[rstest]
fn test_greeks_initialized_false_when_only_one_leg_ready() {
    let mut strategy = create_selected_strategy();
    strategy.call_delta = 0.25;
    strategy.call_delta_ready = true;
    assert!(!strategy.greeks_initialized());
}

#[rstest]
fn test_greeks_initialized_true_when_both_legs_ready() {
    let strategy = create_initialized_strategy();
    assert!(strategy.greeks_initialized());
}

#[rstest]
fn test_should_rehedge_false_when_greeks_not_initialized() {
    let mut strategy = create_strategy();
    strategy.hedge_position = 10.0;
    assert!(!strategy.should_rehedge());
}

#[rstest]
fn test_hedge_pending_default_false() {
    let strategy = create_strategy();
    assert!(!strategy.hedge_pending);
}

#[rstest]
fn test_hedge_pending_blocks_rehedge() {
    let mut strategy = create_initialized_strategy();
    strategy.hedge_position = 5.0;
    assert!(strategy.should_rehedge());

    strategy.hedge_pending = true;
    assert!(strategy.hedge_pending);
}

#[rstest]
fn test_on_option_greeks_updates_call_delta() {
    let mut strategy = create_selected_strategy();
    let call_id = strategy.call_instrument_id.unwrap();

    let greeks = OptionGreeks {
        instrument_id: call_id,
        greeks: OptionGreekValues {
            delta: 0.35,
            gamma: 0.001,
            vega: 0.5,
            theta: -0.1,
            rho: 0.0,
        },
        ..Default::default()
    };

    strategy.on_option_greeks(&greeks).unwrap();
    assert!((strategy.call_delta - 0.35).abs() < 1e-10);
    assert!(strategy.call_delta_ready);
    assert!(!strategy.greeks_initialized());
}

#[rstest]
fn test_on_option_greeks_updates_put_delta() {
    let mut strategy = create_selected_strategy();
    let put_id = strategy.put_instrument_id.unwrap();

    let greeks = OptionGreeks {
        instrument_id: put_id,
        greeks: OptionGreekValues {
            delta: -0.35,
            gamma: 0.001,
            vega: 0.5,
            theta: -0.1,
            rho: 0.0,
        },
        ..Default::default()
    };

    strategy.on_option_greeks(&greeks).unwrap();
    assert!((strategy.put_delta - (-0.35)).abs() < 1e-10);
    assert!(strategy.put_delta_ready);
    assert!(!strategy.greeks_initialized());
}

#[rstest]
fn test_on_option_greeks_initializes_both_legs_before_rehedging() {
    let mut strategy = create_selected_strategy();
    let call_id = strategy.call_instrument_id.unwrap();
    let put_id = strategy.put_instrument_id.unwrap();

    let call_greeks = OptionGreeks {
        instrument_id: call_id,
        greeks: OptionGreekValues {
            delta: 0.25,
            ..Default::default()
        },
        ..Default::default()
    };
    let put_greeks = OptionGreeks {
        instrument_id: put_id,
        greeks: OptionGreekValues {
            delta: -0.22,
            ..Default::default()
        },
        ..Default::default()
    };

    strategy.on_option_greeks(&call_greeks).unwrap();
    assert!(!strategy.greeks_initialized());

    strategy.on_option_greeks(&put_greeks).unwrap();
    assert!(strategy.greeks_initialized());
}

#[rstest]
fn test_rehedge_skips_quantity_rounded_to_zero() {
    let mut strategy = create_initialized_strategy();
    strategy.config.rehedge_delta_threshold = 0.1;
    strategy.hedge_position = 0.4;
    register_strategy(&mut strategy);
    strategy
        .core
        .cache_rc()
        .borrow_mut()
        .add_instrument(InstrumentAny::CryptoPerpetual(hedge_swap_integer_sized()))
        .unwrap();

    assert!(strategy.should_rehedge());

    let event = TimeEvent::new(
        Ustr::from("delta_rehedge"),
        UUID4::new(),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    DataActor::on_time_event(&mut strategy, &event).unwrap();

    // The latch stays clear, so a later rehedge at an expressible size is not blocked.
    assert!(!strategy.hedge_pending);
}

#[rstest]
fn test_greeks_for_unknown_instrument_ignored() {
    let mut strategy = create_selected_strategy();
    let unknown_id = InstrumentId::from("ETH-USD-260327-5000-C.OKX");

    let greeks = OptionGreeks {
        instrument_id: unknown_id,
        greeks: OptionGreekValues {
            delta: 0.99,
            ..Default::default()
        },
        ..Default::default()
    };

    let original_call = strategy.call_delta;
    let original_put = strategy.put_delta;
    strategy.on_option_greeks(&greeks).unwrap();

    assert!((strategy.call_delta - original_call).abs() < 1e-10);
    assert!((strategy.put_delta - original_put).abs() < 1e-10);
    assert!(!strategy.call_delta_ready);
    assert!(!strategy.put_delta_ready);
}

#[rstest]
fn test_on_stop_leaves_positions_unchanged() {
    let mut strategy = create_initialized_strategy();
    let call_id = strategy.call_instrument_id.unwrap();
    let put_id = strategy.put_instrument_id.unwrap();
    register_strategy(&mut strategy);

    strategy.subscribed_greeks = vec![call_id, put_id];
    strategy.call_position = -5.0;
    strategy.put_position = -5.0;
    strategy.hedge_position = 0.75;
    strategy.hedge_pending = true;

    strategy.on_stop().unwrap();

    assert_eq!(strategy.call_position, -5.0);
    assert_eq!(strategy.put_position, -5.0);
    assert_eq!(strategy.hedge_position, 0.75);
    assert!(strategy.subscribed_greeks.is_empty());
    assert!(!strategy.hedge_pending);
}

#[rstest]
fn test_fill_on_unknown_instrument_ignored() {
    let mut strategy = create_initialized_strategy();
    let original_call = strategy.call_position;
    let original_put = strategy.put_position;
    let original_hedge = strategy.hedge_position;

    let unknown_id = InstrumentId::from("ETH-USD-SWAP.OKX");

    let signed_qty = 1.0;

    if unknown_id == strategy.config.hedge_instrument_id {
        strategy.hedge_position += signed_qty;
    } else if Some(unknown_id) == strategy.call_instrument_id {
        strategy.call_position += signed_qty;
    } else if Some(unknown_id) == strategy.put_instrument_id {
        strategy.put_position += signed_qty;
    }

    assert!((strategy.call_position - original_call).abs() < 1e-10);
    assert!((strategy.put_position - original_put).abs() < 1e-10);
    assert!((strategy.hedge_position - original_hedge).abs() < 1e-10);
}

#[rstest]
fn test_on_order_rejected_clears_hedge_pending() {
    let mut strategy = create_strategy();
    strategy.hedge_pending = true;

    strategy.on_order_rejected(make_order_rejected(strategy.config.hedge_instrument_id));

    assert!(!strategy.hedge_pending);
}

#[rstest]
fn test_on_order_rejected_keeps_hedge_pending_for_other_instrument() {
    let mut strategy = create_strategy();
    strategy.hedge_pending = true;

    strategy.on_order_rejected(make_order_rejected(InstrumentId::from(
        "BTC-USD-260327-75000-C.OKX",
    )));

    assert!(strategy.hedge_pending);
}

#[rstest]
fn test_on_order_denied_clears_hedge_pending() {
    let mut strategy = create_strategy();
    strategy.hedge_pending = true;

    strategy.on_order_denied(make_order_denied(strategy.config.hedge_instrument_id));

    assert!(!strategy.hedge_pending);
}

#[rstest]
fn test_on_order_expired_clears_hedge_pending() {
    let mut strategy = create_strategy();
    strategy.hedge_pending = true;

    strategy.on_order_expired(make_order_expired(strategy.config.hedge_instrument_id));

    assert!(!strategy.hedge_pending);
}

#[rstest]
fn test_delta_drift_crosses_threshold_boundary() {
    let mut strategy = create_initialized_strategy();
    strategy.call_position = -10.0;
    strategy.put_position = -10.0;

    assert!(!strategy.should_rehedge());

    strategy.call_delta = 0.22;

    assert!((strategy.portfolio_delta() - (-0.2)).abs() < 1e-10);
    assert!(!strategy.should_rehedge());

    strategy.call_delta = 0.28;

    assert!((strategy.portfolio_delta() - (-0.8)).abs() < 1e-10);
    assert!(strategy.should_rehedge());

    strategy.hedge_position = 0.8;
    assert!(strategy.portfolio_delta().abs() < 1e-10);
    assert!(!strategy.should_rehedge());

    strategy.call_delta = 0.12;

    assert!((strategy.portfolio_delta() - 1.6).abs() < 1e-10);
    assert!(strategy.should_rehedge());
}

#[rstest]
fn test_should_enter_strangle_true_when_ready() {
    let mut strategy = create_entry_ready_strategy();
    register_strategy(&mut strategy);
    assert!(strategy.should_enter_strangle());
}

#[rstest]
fn test_should_enter_strangle_false_when_config_disabled() {
    let mut config = create_config();
    config.enter_strangle = false;
    let mut s = DeltaNeutralVol::new(config);
    s.call_instrument_id = Some(InstrumentId::from("BTC-USD-260327-75000-C.OKX"));
    s.put_instrument_id = Some(InstrumentId::from("BTC-USD-260327-65000-P.OKX"));
    s.call_delta = 0.20;
    s.put_delta = -0.20;
    s.call_delta_ready = true;
    s.put_delta_ready = true;
    s.call_mark_iv = Some(0.55);
    s.put_mark_iv = Some(0.50);
    assert!(!s.should_enter_strangle());
}

#[rstest]
fn test_should_enter_strangle_false_with_existing_call_position() {
    let mut strategy = create_entry_ready_strategy();
    strategy.call_position = -5.0;
    assert!(!strategy.should_enter_strangle());
}

#[rstest]
fn test_should_enter_strangle_false_with_existing_put_position() {
    let mut strategy = create_entry_ready_strategy();
    strategy.put_position = -5.0;
    assert!(!strategy.should_enter_strangle());
}

#[rstest]
fn test_should_enter_strangle_false_without_call_mark_iv() {
    let mut strategy = create_initialized_strategy();
    strategy.put_mark_iv = Some(0.50);
    assert!(!strategy.should_enter_strangle());
}

#[rstest]
fn test_should_enter_strangle_false_without_put_mark_iv() {
    let mut strategy = create_initialized_strategy();
    strategy.call_mark_iv = Some(0.55);
    assert!(!strategy.should_enter_strangle());
}

#[rstest]
fn test_should_enter_strangle_false_without_greeks_initialized() {
    let mut strategy = create_selected_strategy();
    strategy.call_mark_iv = Some(0.55);
    strategy.put_mark_iv = Some(0.50);
    assert!(!strategy.should_enter_strangle());
}

#[rstest]
fn test_should_enter_strangle_with_premium_mode_waits_for_quotes() {
    let mut config = create_config();
    config.entry_premium_offset_ticks = Some(1);
    let mut strategy = DeltaNeutralVol::new(config);
    let call_id = InstrumentId::from("BTC-USD-260327-75000-C.OKX");
    let put_id = InstrumentId::from("BTC-USD-260327-65000-P.OKX");

    strategy.call_instrument_id = Some(call_id);
    strategy.put_instrument_id = Some(put_id);
    strategy.call_delta = 0.20;
    strategy.put_delta = -0.20;
    strategy.call_delta_ready = true;
    strategy.put_delta_ready = true;
    register_strategy(&mut strategy);

    assert!(!strategy.entry_price_data_ready());
    assert!(!strategy.should_enter_strangle());

    strategy.call_quote = Some(quote_tick(call_id, "10.0", "10.5"));

    assert!(!strategy.entry_price_data_ready());
    assert!(!strategy.should_enter_strangle());

    strategy.put_quote = Some(quote_tick(put_id, "8.0", "8.5"));

    assert!(strategy.entry_price_data_ready());
    assert!(strategy.should_enter_strangle());
}

#[rstest]
fn test_should_enter_strangle_false_after_entry_attempted() {
    let mut strategy = create_entry_ready_strategy();
    register_strategy(&mut strategy);

    assert!(strategy.should_enter_strangle());

    strategy.entry_attempted = true;

    assert!(!strategy.should_enter_strangle());
}

#[rstest]
fn test_config_enter_strangle_default_true() {
    let config = create_config();
    assert!(config.enter_strangle);
    assert_eq!(config.entry_iv_offset, 0.0);
    assert!(config.entry_premium_offset_ticks.is_none());
}

#[rstest]
fn test_config_entry_builder_methods() {
    let config = DeltaNeutralVolConfig::builder()
        .option_family("BTC-USD".to_string())
        .hedge_instrument_id(InstrumentId::from("BTC-USD-SWAP.OKX"))
        .client_id(ClientId::new("OKX"))
        .enter_strangle(false)
        .entry_iv_offset(0.05)
        .entry_time_in_force(TimeInForce::Ioc)
        .entry_premium_offset_ticks(2)
        .build();

    assert!(!config.enter_strangle);
    assert_eq!(config.entry_iv_offset, 0.05);
    assert_eq!(config.entry_time_in_force, TimeInForce::Ioc);
    assert_eq!(config.entry_premium_offset_ticks, Some(2));
}

#[rstest]
fn test_on_option_greeks_stores_mark_iv() {
    let mut strategy = create_selected_strategy();
    let call_id = strategy.call_instrument_id.unwrap();

    let greeks = OptionGreeks {
        instrument_id: call_id,
        greeks: OptionGreekValues {
            delta: 0.25,
            ..Default::default()
        },
        mark_iv: Some(0.55),
        ..Default::default()
    };

    strategy.on_option_greeks(&greeks).unwrap();
    assert_eq!(strategy.call_mark_iv, Some(0.55));
}

#[rstest]
fn test_on_option_greeks_stores_put_mark_iv() {
    let mut strategy = create_selected_strategy();
    let put_id = strategy.put_instrument_id.unwrap();

    let greeks = OptionGreeks {
        instrument_id: put_id,
        greeks: OptionGreekValues {
            delta: -0.25,
            ..Default::default()
        },
        mark_iv: Some(0.50),
        ..Default::default()
    };

    strategy.on_option_greeks(&greeks).unwrap();
    assert_eq!(strategy.put_mark_iv, Some(0.50));
}

#[rstest]
fn test_on_option_greeks_preserves_mark_iv_when_none() {
    let mut strategy = create_selected_strategy();
    strategy.call_mark_iv = Some(0.55);
    let call_id = strategy.call_instrument_id.unwrap();

    let greeks = OptionGreeks {
        instrument_id: call_id,
        greeks: OptionGreekValues {
            delta: 0.30,
            ..Default::default()
        },
        mark_iv: None,
        ..Default::default()
    };

    strategy.on_option_greeks(&greeks).unwrap();
    assert_eq!(strategy.call_mark_iv, Some(0.55));
    assert!((strategy.call_delta - 0.30).abs() < 1e-10);
}

#[rstest]
fn test_mark_iv_default_none() {
    let strategy = create_strategy();
    assert!(strategy.call_mark_iv.is_none());
    assert!(strategy.put_mark_iv.is_none());
}

#[rstest]
fn test_on_reset_restores_constructor_state() {
    use nautilus_common::component::Component;

    let mut strategy = create_initialized_strategy();
    let expected_config = format!("{:?}", strategy.config);
    register_strategy(&mut strategy);
    strategy.initialize().unwrap();
    let call_id = strategy.call_instrument_id.unwrap();
    let put_id = strategy.put_instrument_id.unwrap();
    strategy.subscribed_greeks = vec![call_id, put_id];
    strategy.call_delta = 0.31;
    strategy.put_delta = -0.42;
    strategy.call_mark_iv = Some(0.55);
    strategy.put_mark_iv = Some(0.60);
    strategy.call_quote = Some(quote_tick(call_id, "100.0", "101.0"));
    strategy.put_quote = Some(quote_tick(put_id, "90.0", "91.0"));
    strategy.call_greeks = Some(OptionGreeks {
        instrument_id: call_id,
        ..Default::default()
    });

    strategy.put_greeks = Some(OptionGreeks {
        instrument_id: put_id,
        ..Default::default()
    });

    strategy.call_position = -2.0;
    strategy.put_position = -3.0;
    strategy.hedge_position = 4.0;
    strategy
        .order_filled_qty
        .insert(ClientOrderId::from("O-RESET"), 7.into());
    strategy.hedge_pending = true;
    strategy.entry_attempted = true;

    strategy.reset().unwrap();

    assert_eq!(strategy.call_instrument_id, None);
    assert_eq!(strategy.put_instrument_id, None);
    assert!(strategy.subscribed_greeks.is_empty());
    assert_eq!(strategy.call_delta, 0.0);
    assert_eq!(strategy.put_delta, 0.0);
    assert_eq!(strategy.call_mark_iv, None);
    assert_eq!(strategy.put_mark_iv, None);
    assert_eq!(strategy.call_quote, None);
    assert_eq!(strategy.put_quote, None);
    assert_eq!(strategy.call_greeks, None);
    assert_eq!(strategy.put_greeks, None);
    assert!(!strategy.call_delta_ready);
    assert!(!strategy.put_delta_ready);
    assert_eq!(strategy.call_position, 0.0);
    assert_eq!(strategy.put_position, 0.0);
    assert_eq!(strategy.hedge_position, 0.0);
    assert!(strategy.order_filled_qty.is_empty());
    assert!(!strategy.hedge_pending);
    assert!(!strategy.entry_attempted);
    assert_eq!(format!("{:?}", strategy.config), expected_config);
    assert!(strategy.is_ready());
}

#[rstest]
fn test_full_hedge_fill_void_restores_timer_hedge() {
    let mut strategy = create_initialized_strategy();
    register_strategy(&mut strategy);
    let instrument = InstrumentAny::CryptoPerpetual(hedge_swap_integer_sized());
    let strategy_id = strategy.strategy_id().unwrap();
    let fill = OrderFilledSpec::builder()
        .strategy_id(strategy_id)
        .instrument_id(instrument.id())
        .client_order_id(ClientOrderId::from("O-HEDGE-VOID"))
        .position_id(PositionId::from("P-HEDGE-VOID"))
        .last_qty(Quantity::from(2))
        .last_px(Price::from("70000.0"))
        .build();
    let mut position = Position::new(&instrument, fill.clone());
    let order = filled_order(&fill);
    let cache = strategy.core.cache_rc();
    {
        let mut cache = cache.borrow_mut();
        cache.add_instrument(instrument).unwrap();
        cache.add_position(&position, OmsType::Hedging).unwrap();
        cache
            .add_order(order, fill.position_id, None, true)
            .unwrap();
    }

    strategy.call_position = -10.0;
    strategy.hedge_pending = true;
    strategy.on_order_filled(&fill);
    assert_eq!(strategy.hedge_position, 2.0);
    assert_eq!(strategy.portfolio_delta(), 0.0);
    assert!(!strategy.hedge_pending);

    let voided = OrderFillVoidedSpec::builder()
        .strategy_id(strategy_id)
        .instrument_id(fill.instrument_id)
        .client_order_id(fill.client_order_id)
        .trade_id(fill.trade_id)
        .position_id(position.id)
        .voided_qty(fill.last_qty)
        .last_px(fill.last_px)
        .build();
    position
        .apply_fill_void(voided.clone(), fill.last_qty, None)
        .unwrap();
    {
        let mut cache = cache.borrow_mut();
        cache.update_position(&position).unwrap();
        cache
            .update_order(&OrderEventAny::FillVoided(voided.clone()))
            .unwrap();
    }

    strategy.on_order_fill_voided(&voided);
    assert_eq!(position.signed_qty, 0.into());
    assert_eq!(strategy.hedge_position, 0.0);
    assert_eq!(strategy.portfolio_delta(), -2.0);

    let (handler, commands) = get_typed_into_message_saving_handler::<TradingCommand>(None);
    msgbus::register_trading_command_endpoint(
        MessagingSwitchboard::risk_engine_queue_execute(),
        handler,
    );

    let timer = TimeEvent::new(
        Ustr::from("delta_rehedge"),
        UUID4::new(),
        0.into(),
        0.into(),
    );

    DataActor::on_time_event(&mut strategy, &timer).unwrap();
    let messages = commands.get_messages();
    assert_eq!(messages.len(), 1);

    let TradingCommand::SubmitOrder(command) = &messages[0] else {
        panic!("Expected submit order");
    };

    let cache = cache.borrow();
    let hedge = cache.order(&command.client_order_id).unwrap();
    assert_eq!(hedge.instrument_id(), fill.instrument_id);
    assert_eq!(hedge.strategy_id(), strategy_id);
    assert_eq!(hedge.order_type(), OrderType::Market);
    assert_eq!(hedge.order_side(), OrderSide::Buy);
    assert_eq!(hedge.quantity(), Quantity::from(2));
    assert_eq!(command.client_id, Some(strategy.config.client_id));
    assert!(strategy.hedge_pending);
}

#[rstest]
#[case(0, [-6.0, -8.0, 7.0], [-2.0, -8.0, 7.0])]
#[case(1, [-6.0, -8.0, 7.0], [-6.0, -3.0, 7.0])]
#[case(2, [-6.0, -8.0, 7.0], [-6.0, -8.0, 1.0])]
fn test_fill_void_corrects_only_affected_leg_and_preserves_external_baseline(
    #[case] leg: usize,
    #[case] mut initial: [f64; 3],
    #[case] mut expected: [f64; 3],
) {
    let (mut strategy, mut positions) = start_strategy_with_positions();
    assert_eq!(
        [
            strategy.call_position,
            strategy.put_position,
            strategy.hedge_position
        ],
        initial
    );

    for position in &positions {
        assert_eq!(
            strategy.order_filled_qty[&position.opening_order_id],
            position.events[0].last_qty.as_decimal()
        );
    }

    let position = &mut positions[leg];
    let fill = position.events[0].clone();
    let cache = strategy.core.cache_rc();
    let instrument = cache
        .borrow()
        .instrument(&fill.instrument_id)
        .unwrap()
        .clone();
    let foreign_fill = OrderFilledSpec::builder()
        .strategy_id(StrategyId::from("OTHER-003"))
        .instrument_id(fill.instrument_id)
        .client_order_id(ClientOrderId::from("O-LATER-FOREIGN"))
        .position_id(PositionId::from("P-LATER-FOREIGN"))
        .last_qty(Quantity::from(9))
        .last_px(fill.last_px)
        .build();
    let foreign = Position::new(&instrument, foreign_fill);
    cache
        .borrow_mut()
        .add_position(&foreign, OmsType::Hedging)
        .unwrap();

    let own_fill = OrderFilledSpec::builder()
        .strategy_id(fill.strategy_id)
        .instrument_id(fill.instrument_id)
        .client_order_id(ClientOrderId::from("O-LATER-OWN"))
        .position_id(PositionId::from("P-LATER-OWN"))
        .last_qty(Quantity::from(2))
        .last_px(fill.last_px)
        .build();
    let own = Position::new(&instrument, own_fill.clone());
    cache
        .borrow_mut()
        .add_position(&own, OmsType::Hedging)
        .unwrap();
    strategy.on_order_filled(&own_fill);
    initial[leg] += 2.0;
    expected[leg] += 2.0;
    strategy.hedge_pending = true;

    let mut voided = OrderFillVoidedSpec::builder()
        .strategy_id(fill.strategy_id)
        .instrument_id(fill.instrument_id)
        .client_order_id(fill.client_order_id)
        .trade_id(fill.trade_id)
        .order_side(fill.order_side)
        .position_id(position.id)
        .voided_qty(Quantity::from(1))
        .last_px(fill.last_px)
        .build();
    position
        .apply_fill_void(voided.clone(), Quantity::from(1), None)
        .unwrap();
    cache.borrow_mut().update_position(position).unwrap();
    cache
        .borrow_mut()
        .update_order(&OrderEventAny::FillVoided(voided.clone()))
        .unwrap();
    strategy.on_order_fill_voided(&voided);
    let mut partial = initial;
    partial[leg] += if fill.order_side == OrderSide::Buy {
        -1.0
    } else {
        1.0
    };

    assert_eq!(
        [
            strategy.call_position,
            strategy.put_position,
            strategy.hedge_position
        ],
        partial
    );

    voided.voided_qty = fill.last_qty;
    voided.correction_id = Ustr::from("CORRECTION-FULL");
    position
        .apply_fill_void(voided.clone(), fill.last_qty, None)
        .unwrap();
    cache.borrow_mut().update_position(position).unwrap();
    cache
        .borrow_mut()
        .update_order(&OrderEventAny::FillVoided(voided.clone()))
        .unwrap();
    strategy.on_order_fill_voided(&voided);
    strategy.on_order_fill_voided(&voided);
    assert_eq!(
        [
            strategy.call_position,
            strategy.put_position,
            strategy.hedge_position
        ],
        expected
    );
    assert_eq!(strategy.order_filled_qty[&fill.client_order_id], 0.into());
    assert!(strategy.hedge_pending);
}

fn start_strategy_with_positions() -> (DeltaNeutralVol, [Position; 3]) {
    let mut strategy = create_selected_strategy();
    strategy.config.option_family = "BTC".to_string();
    register_strategy(&mut strategy);
    let mut call = crypto_option_btc_deribit(3, 1, Price::from("0.001"), Quantity::from("0.1"));
    call.id = strategy.call_instrument_id.unwrap();
    call.option_kind = OptionKind::Call;
    let mut put = crypto_option_btc_deribit(3, 1, Price::from("0.001"), Quantity::from("0.1"));
    put.id = strategy.put_instrument_id.unwrap();

    let instruments = [
        InstrumentAny::CryptoOption(call),
        InstrumentAny::CryptoOption(put),
        InstrumentAny::CryptoPerpetual(hedge_swap_integer_sized()),
    ];

    let strategy_id = strategy.strategy_id().unwrap();
    let cache = strategy.core.cache_rc();
    let mut own_positions = Vec::new();

    for (leg, instrument) in instruments.iter().enumerate() {
        cache
            .borrow_mut()
            .add_instrument(instrument.clone())
            .unwrap();

        let side = if leg == 2 {
            OrderSide::Buy
        } else {
            OrderSide::Sell
        };

        for (owner, qty) in [
            (StrategyId::from("EXTERNAL-002"), [2, 3, 1][leg]),
            (strategy_id, [4, 5, 6][leg]),
        ] {
            let client_order_id = format!("O-{leg}-{owner}");
            let position_id = format!("P-{leg}-{owner}");
            let fill = OrderFilledSpec::builder()
                .strategy_id(owner)
                .instrument_id(instrument.id())
                .client_order_id(ClientOrderId::new(client_order_id))
                .position_id(PositionId::new(position_id))
                .last_qty(Quantity::from(qty))
                .last_px(Price::from("100.0"))
                .order_side(side)
                .build();
            let order = filled_order(&fill);
            cache
                .borrow_mut()
                .add_order(order, fill.position_id, None, true)
                .unwrap();
            let position = Position::new(instrument, fill);
            cache
                .borrow_mut()
                .add_position(&position, OmsType::Hedging)
                .unwrap();

            if owner == strategy_id {
                own_positions.push(position);
            }
        }
    }

    strategy
        .core
        .clock_mut()
        .register_default_handler(TimeEventCallback::from(|_: TimeEvent| {}));
    DataActor::on_start(&mut strategy).unwrap();
    (strategy, own_positions.try_into().unwrap())
}

#[rstest]
fn test_fill_void_ignores_other_strategy_fills_in_shared_venue_position() {
    let (mut strategy, mut positions) = start_strategy_with_positions();
    let position = &mut positions[2];
    let instrument_id = position.instrument_id;
    let cache = strategy.core.cache_rc();
    let foreign_fill = OrderFilledSpec::builder()
        .strategy_id(StrategyId::from("OTHER-003"))
        .instrument_id(instrument_id)
        .position_id(position.id)
        .client_order_id(ClientOrderId::from("O-SHARED-FOREIGN"))
        .trade_id(TradeId::from("T-SHARED-FOREIGN"))
        .last_qty(Quantity::from(9))
        .last_px(Price::from("100.0"))
        .build();
    position.apply(&foreign_fill);
    let own_fill = OrderFilledSpec::builder()
        .strategy_id(strategy.strategy_id().unwrap())
        .instrument_id(instrument_id)
        .position_id(position.id)
        .client_order_id(ClientOrderId::from("O-SHARED-OWN"))
        .trade_id(TradeId::from("T-SHARED-OWN"))
        .last_qty(Quantity::from(2))
        .last_px(Price::from("100.0"))
        .build();
    position.apply(&own_fill);
    let order = filled_order(&own_fill);
    cache
        .borrow_mut()
        .add_order(order, Some(position.id), None, true)
        .unwrap();
    cache.borrow_mut().update_position(position).unwrap();
    strategy.on_order_filled(&own_fill);
    assert_eq!(position.signed_qty, 17.into());
    assert_eq!(strategy.hedge_position, 9.0);

    let voided = OrderFillVoidedSpec::builder()
        .strategy_id(own_fill.strategy_id)
        .instrument_id(instrument_id)
        .position_id(position.id)
        .client_order_id(own_fill.client_order_id)
        .trade_id(own_fill.trade_id)
        .voided_qty(own_fill.last_qty)
        .last_px(own_fill.last_px)
        .build();
    position
        .apply_fill_void(voided.clone(), own_fill.last_qty, None)
        .unwrap();
    cache.borrow_mut().update_position(position).unwrap();
    cache
        .borrow_mut()
        .update_order(&OrderEventAny::FillVoided(voided.clone()))
        .unwrap();
    strategy.on_order_fill_voided(&voided);
    assert_eq!(position.signed_qty, 15.into());
    assert_eq!(strategy.call_position, -6.0);
    assert_eq!(strategy.put_position, -8.0);
    assert_eq!(strategy.hedge_position, 7.0);
    assert_eq!(
        strategy.order_filled_qty[&own_fill.client_order_id],
        0.into()
    );
}

#[rstest]
fn test_timer_drops_applied_quantities_for_purged_orders() {
    let (mut strategy, positions) = start_strategy_with_positions();
    let purged_id = positions[0].opening_order_id;
    strategy
        .core
        .cache_rc()
        .borrow_mut()
        .purge_order(purged_id, false)
        .unwrap();

    let timer = TimeEvent::new(
        Ustr::from("delta_rehedge"),
        UUID4::new(),
        0.into(),
        0.into(),
    );

    DataActor::on_time_event(&mut strategy, &timer).unwrap();
    assert_eq!(strategy.order_filled_qty.len(), 2);
    assert!(!strategy.order_filled_qty.contains_key(&purged_id));
    assert_eq!(
        strategy.order_filled_qty[&positions[1].opening_order_id],
        5.into()
    );
    assert_eq!(
        strategy.order_filled_qty[&positions[2].opening_order_id],
        6.into()
    );
    assert_eq!(
        [
            strategy.call_position,
            strategy.put_position,
            strategy.hedge_position
        ],
        [-6.0, -8.0, 7.0]
    );
}

#[rstest]
fn test_fill_void_preserves_other_fills_on_same_order() {
    let mut strategy = create_initialized_strategy();
    register_strategy(&mut strategy);
    let instrument = InstrumentAny::CryptoPerpetual(hedge_swap_integer_sized());
    let strategy_id = strategy.strategy_id().unwrap();
    let first = OrderFilledSpec::builder()
        .strategy_id(strategy_id)
        .instrument_id(instrument.id())
        .client_order_id(ClientOrderId::from("O-PARTIAL-FILLS"))
        .position_id(PositionId::from("P-PARTIAL-FILLS"))
        .trade_id(TradeId::from("T-FIRST"))
        .last_qty(Quantity::from(2))
        .last_px(Price::from("100.0"))
        .build();
    let mut second = first.clone();
    second.trade_id = TradeId::from("T-SECOND");
    second.event_id = UUID4::new();
    second.last_qty = Quantity::from(3);
    let order = OrderTestBuilder::new(OrderType::Market)
        .strategy_id(strategy_id)
        .instrument_id(instrument.id())
        .client_order_id(first.client_order_id)
        .side(OrderSide::Buy)
        .quantity(Quantity::from(5))
        .submit(true)
        .build();
    let cache = strategy.core.cache_rc();
    cache
        .borrow_mut()
        .add_order(order, first.position_id, None, true)
        .unwrap();
    let mut position = Position::new(&instrument, first.clone());
    cache
        .borrow_mut()
        .add_position(&position, OmsType::Hedging)
        .unwrap();
    cache
        .borrow_mut()
        .update_order(&OrderEventAny::Filled(first.clone()))
        .unwrap();
    strategy.hedge_pending = true;
    strategy.on_order_filled(&first);
    assert_eq!(strategy.hedge_position, 2.0);
    assert_eq!(strategy.order_filled_qty[&first.client_order_id], 2.into());
    assert!(strategy.hedge_pending);

    position.apply(&second);
    cache.borrow_mut().update_position(&position).unwrap();
    cache
        .borrow_mut()
        .update_order(&OrderEventAny::Filled(second.clone()))
        .unwrap();
    strategy.on_order_filled(&second);
    assert_eq!(strategy.hedge_position, 5.0);
    assert_eq!(strategy.order_filled_qty[&first.client_order_id], 5.into());
    assert!(!strategy.hedge_pending);

    let voided = OrderFillVoidedSpec::builder()
        .strategy_id(strategy_id)
        .instrument_id(first.instrument_id)
        .position_id(position.id)
        .client_order_id(first.client_order_id)
        .trade_id(first.trade_id)
        .voided_qty(first.last_qty)
        .last_px(first.last_px)
        .build();
    position
        .apply_fill_void(voided.clone(), first.last_qty, None)
        .unwrap();
    cache.borrow_mut().update_position(&position).unwrap();
    cache
        .borrow_mut()
        .update_order(&OrderEventAny::FillVoided(voided.clone()))
        .unwrap();
    strategy.on_order_fill_voided(&voided);
    assert_eq!(position.signed_qty, 3.into());
    assert_eq!(strategy.hedge_position, 3.0);
    assert_eq!(strategy.order_filled_qty[&first.client_order_id], 3.into());
    assert!(!strategy.hedge_pending);
}

#[rstest]
fn test_fill_void_without_cached_order_preserves_exposure(#[values(false, true)] tracked: bool) {
    let mut strategy = create_initialized_strategy();
    register_strategy(&mut strategy);
    strategy.call_position = -2.0;
    strategy.put_position = -3.0;
    strategy.hedge_position = 4.0;
    strategy.hedge_pending = true;
    let client_order_id = ClientOrderId::from("O-NOT-CACHED");
    if tracked {
        strategy.order_filled_qty.insert(client_order_id, 6.into());
    }

    let voided = OrderFillVoidedSpec::builder()
        .instrument_id(strategy.config.hedge_instrument_id)
        .client_order_id(client_order_id)
        .build();
    strategy.on_order_fill_voided(&voided);
    assert_eq!(
        [
            strategy.call_position,
            strategy.put_position,
            strategy.hedge_position
        ],
        [-2.0, -3.0, 4.0]
    );
    assert_eq!(
        strategy.order_filled_qty.get(&client_order_id).copied(),
        tracked.then_some(6.into())
    );
    assert!(strategy.hedge_pending);
}

fn filled_order(fill: &OrderFilled) -> OrderAny {
    let mut order = OrderTestBuilder::new(OrderType::Market)
        .strategy_id(fill.strategy_id)
        .instrument_id(fill.instrument_id)
        .client_order_id(fill.client_order_id)
        .side(fill.order_side)
        .quantity(fill.last_qty)
        .submit(true)
        .build();
    order.apply(OrderEventAny::Filled(fill.clone())).unwrap();
    order
}

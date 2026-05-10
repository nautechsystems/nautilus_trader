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
    clock::{Clock, TestClock},
};
use nautilus_model::{
    data::{greeks::OptionGreekValues, option_chain::OptionGreeks},
    enums::{OrderSide, TimeInForce},
    identifiers::{ClientId, InstrumentId, StrategyId, TraderId},
};
use nautilus_portfolio::portfolio::Portfolio;
use rstest::rstest;

use super::{DeltaNeutralVol, DeltaNeutralVolConfig};
use crate::strategy::Strategy;

fn create_config() -> DeltaNeutralVolConfig {
    DeltaNeutralVolConfig::new(
        "BTC-USD".to_string(),
        InstrumentId::from("BTC-USD-SWAP.OKX"),
        ClientId::new("OKX"),
    )
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

fn register_strategy(strategy: &mut DeltaNeutralVol) {
    let trader_id = TraderId::from("TESTER-001");
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
    let cache = Rc::new(RefCell::new(Cache::default()));
    let portfolio = Rc::new(RefCell::new(Portfolio::new(
        cache.clone(),
        clock.clone(),
        None,
    )));

    strategy
        .core
        .register(trader_id, clock, cache, portfolio)
        .unwrap();
}

#[rstest]
fn test_new_sets_strategy_id() {
    let strategy = create_strategy();
    assert_eq!(
        strategy.core().config.strategy_id,
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
}

#[rstest]
fn test_config_builder_methods() {
    let config = create_config()
        .with_target_call_delta(0.30)
        .with_target_put_delta(-0.30)
        .with_contracts(10)
        .with_rehedge_delta_threshold(1.0)
        .with_rehedge_interval_secs(60)
        .with_expiry_filter("260327".to_string())
        .with_strategy_id(StrategyId::from("CUSTOM-001"))
        .with_order_id_tag("002".to_string());

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
    let config = create_config().with_rehedge_delta_threshold(0.1);
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
    let config = create_config().with_rehedge_delta_threshold(0.1);
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
    let config = create_config().with_enter_strangle(false);
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
fn test_config_enter_strangle_default_true() {
    let config = create_config();
    assert!(config.enter_strangle);
    assert_eq!(config.entry_iv_offset, 0.0);
}

#[rstest]
fn test_config_entry_builder_methods() {
    let config = create_config()
        .with_enter_strangle(false)
        .with_entry_iv_offset(0.05)
        .with_entry_time_in_force(TimeInForce::Ioc);

    assert!(!config.enter_strangle);
    assert_eq!(config.entry_iv_offset, 0.05);
    assert_eq!(config.entry_time_in_force, TimeInForce::Ioc);
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

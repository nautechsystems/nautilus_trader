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

use std::{cell::RefCell, collections::VecDeque, rc::Rc};

use nautilus_common::{
    actor::DataActor,
    cache::Cache,
    clock::{Clock, TestClock},
};
use nautilus_model::{
    data::{Bar, BarSpecification, BarType, TradeTick},
    enums::{AggregationSource, AggressorSide, BarAggregation, PriceType},
    identifiers::{ClientOrderId, InstrumentId, StrategyId, Symbol, TradeId, TraderId},
    instruments::{CryptoPerpetual, InstrumentAny},
    types::{Currency, Price, Quantity},
};
use nautilus_portfolio::portfolio::Portfolio;
use rstest::rstest;
use rust_decimal_macros::dec;

use super::{HurstVpinDirectional, HurstVpinDirectionalConfig};

fn pf_xbtusd() -> CryptoPerpetual {
    CryptoPerpetual::new(
        InstrumentId::from("PF_XBTUSD.KRAKEN"),
        Symbol::from("PF_XBTUSD"),
        Currency::BTC(),
        Currency::USD(),
        Currency::USD(),
        false,
        1,
        4,
        Price::from("0.5"),
        Quantity::from("0.0001"),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(dec!(0.02)),
        Some(dec!(0.01)),
        Some(dec!(0.0002)),
        Some(dec!(0.0005)),
        None,
        0.into(),
        0.into(),
    )
}

fn bar_type(instrument_id: InstrumentId) -> BarType {
    BarType::new(
        instrument_id,
        BarSpecification::new(2_000_000, BarAggregation::Value, PriceType::Last),
        AggregationSource::External,
    )
}

fn create_strategy(instrument_id: InstrumentId) -> HurstVpinDirectional {
    let config = HurstVpinDirectionalConfig::new(
        instrument_id,
        bar_type(instrument_id),
        Quantity::from("0.01"),
    );
    HurstVpinDirectional::new(config)
}

fn create_strategy_with_windows(
    instrument_id: InstrumentId,
    hurst_window: usize,
    hurst_lags: Vec<usize>,
) -> HurstVpinDirectional {
    let config = HurstVpinDirectionalConfig::new(
        instrument_id,
        bar_type(instrument_id),
        Quantity::from("0.01"),
    )
    .with_hurst_window(hurst_window)
    .with_hurst_lags(hurst_lags);
    HurstVpinDirectional::new(config)
}

fn register_strategy(strategy: &mut HurstVpinDirectional) {
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

fn trade(instrument_id: InstrumentId, size: &str, side: AggressorSide, ts: u64) -> TradeTick {
    TradeTick::new(
        instrument_id,
        Price::from("30000.0"),
        Quantity::from(size),
        side,
        TradeId::from("T-1"),
        ts.into(),
        ts.into(),
    )
}

fn bar(bar_type: BarType, close: &str, ts: u64) -> Bar {
    Bar::new(
        bar_type,
        Price::from(close),
        Price::from(close),
        Price::from(close),
        Price::from(close),
        Quantity::from("1"),
        ts.into(),
        ts.into(),
    )
}

#[rstest]
fn test_new_initializes_clean_state() {
    let strategy = create_strategy(InstrumentId::from("PF_XBTUSD.KRAKEN"));

    assert!(strategy.hurst.is_none());
    assert!(strategy.vpin.is_none());
    assert!(strategy.signed_vpin.is_none());
    assert!(strategy.last_close.is_none());
    assert_eq!(strategy.bucket_buy_volume, 0.0);
    assert_eq!(strategy.bucket_sell_volume, 0.0);
    assert!(!strategy.exit_cooldown);
    assert!(strategy.entry_order_id.is_none());
    assert!(strategy.exit_order_ids.is_empty());
    assert!(strategy.position_opened_ns.is_none());
}

#[rstest]
fn test_config_defaults() {
    let instrument_id = InstrumentId::from("PF_XBTUSD.KRAKEN");
    let strategy = create_strategy(instrument_id);
    let config = &strategy.config;

    assert_eq!(config.hurst_window, 128);
    assert_eq!(config.hurst_lags, vec![4, 8, 16, 32]);
    assert_eq!(config.hurst_enter, 0.55);
    assert_eq!(config.hurst_exit, 0.50);
    assert_eq!(config.vpin_window, 50);
    assert_eq!(config.vpin_threshold, 0.30);
    assert_eq!(config.max_holding_secs, 3600);
    assert_eq!(
        strategy.core.config.strategy_id,
        Some(StrategyId::from("HURST_VPIN-001")),
    );
}

#[rstest]
fn test_buyer_aggressor_adds_to_buy_volume() {
    let mut strategy = create_strategy(InstrumentId::from("PF_XBTUSD.KRAKEN"));

    strategy
        .on_trade(&trade(
            strategy.config.instrument_id,
            "5.0",
            AggressorSide::Buyer,
            1,
        ))
        .unwrap();

    assert_eq!(strategy.bucket_buy_volume, 5.0);
    assert_eq!(strategy.bucket_sell_volume, 0.0);
}

#[rstest]
fn test_seller_aggressor_adds_to_sell_volume() {
    let mut strategy = create_strategy(InstrumentId::from("PF_XBTUSD.KRAKEN"));

    strategy
        .on_trade(&trade(
            strategy.config.instrument_id,
            "7.0",
            AggressorSide::Seller,
            1,
        ))
        .unwrap();

    assert_eq!(strategy.bucket_buy_volume, 0.0);
    assert_eq!(strategy.bucket_sell_volume, 7.0);
}

#[rstest]
fn test_no_aggressor_is_ignored() {
    let mut strategy = create_strategy(InstrumentId::from("PF_XBTUSD.KRAKEN"));

    strategy
        .on_trade(&trade(
            strategy.config.instrument_id,
            "3.0",
            AggressorSide::NoAggressor,
            1,
        ))
        .unwrap();

    assert_eq!(strategy.bucket_buy_volume, 0.0);
    assert_eq!(strategy.bucket_sell_volume, 0.0);
}

#[rstest]
fn test_first_bar_records_close_but_not_return() {
    let mut strategy = create_strategy(InstrumentId::from("PF_XBTUSD.KRAKEN"));
    register_strategy(&mut strategy);
    let cache = Rc::new(RefCell::new(Cache::default()));
    cache
        .borrow_mut()
        .add_instrument(InstrumentAny::CryptoPerpetual(pf_xbtusd()))
        .unwrap();

    let bt = bar_type(strategy.config.instrument_id);
    strategy.on_bar(&bar(bt, "30000.0", 0)).unwrap();

    assert_eq!(strategy.last_close, Some(30000.0));
    assert_eq!(strategy.returns.len(), 0);
}

#[rstest]
fn test_second_bar_appends_log_return() {
    let mut strategy = create_strategy(InstrumentId::from("PF_XBTUSD.KRAKEN"));
    register_strategy(&mut strategy);

    let bt = bar_type(strategy.config.instrument_id);
    strategy.on_bar(&bar(bt, "30000.0", 0)).unwrap();
    strategy.on_bar(&bar(bt, "33000.0", 1)).unwrap();

    assert_eq!(strategy.returns.len(), 1);
    let expected = (33000.0_f64 / 30000.0_f64).ln();
    assert!((strategy.returns[0] - expected).abs() < 1e-9);
}

#[rstest]
fn test_zero_volume_bar_does_not_record_imbalance() {
    let mut strategy = create_strategy(InstrumentId::from("PF_XBTUSD.KRAKEN"));
    register_strategy(&mut strategy);

    let bt = bar_type(strategy.config.instrument_id);
    strategy.on_bar(&bar(bt, "30000.0", 0)).unwrap();
    strategy.on_bar(&bar(bt, "30010.0", 1)).unwrap();

    assert!(strategy.abs_imbalances.is_empty());
    assert!(strategy.signed_imbalances.is_empty());
}

#[rstest]
fn test_bar_finalizes_bucket_and_resets_accumulators() {
    let mut strategy = create_strategy(InstrumentId::from("PF_XBTUSD.KRAKEN"));
    register_strategy(&mut strategy);

    let bt = bar_type(strategy.config.instrument_id);
    strategy.on_bar(&bar(bt, "30000.0", 0)).unwrap();
    strategy
        .on_trade(&trade(
            strategy.config.instrument_id,
            "7.0",
            AggressorSide::Buyer,
            1,
        ))
        .unwrap();
    strategy
        .on_trade(&trade(
            strategy.config.instrument_id,
            "3.0",
            AggressorSide::Seller,
            2,
        ))
        .unwrap();
    strategy.on_bar(&bar(bt, "30010.0", 3)).unwrap();

    assert_eq!(strategy.abs_imbalances.len(), 1);
    assert!((strategy.abs_imbalances[0] - 0.4).abs() < 1e-9);
    assert_eq!(strategy.signed_imbalances.len(), 1);
    assert!((strategy.signed_imbalances[0] - 0.4).abs() < 1e-9);
    assert_eq!(strategy.bucket_buy_volume, 0.0);
    assert_eq!(strategy.bucket_sell_volume, 0.0);
}

#[rstest]
fn test_bar_clears_exit_cooldown() {
    let mut strategy = create_strategy(InstrumentId::from("PF_XBTUSD.KRAKEN"));
    register_strategy(&mut strategy);
    strategy.exit_cooldown = true;

    let bt = bar_type(strategy.config.instrument_id);
    strategy.on_bar(&bar(bt, "30000.0", 0)).unwrap();

    assert!(!strategy.exit_cooldown);
}

#[rstest]
fn test_hurst_returns_none_when_insufficient_returns() {
    let mut strategy = create_strategy(InstrumentId::from("PF_XBTUSD.KRAKEN"));
    // Fewer than hurst_window (128)
    for value in [0.01, -0.01, 0.01] {
        strategy.returns.push_back(value);
    }
    assert!(strategy.estimate_hurst().is_none());
}

#[rstest]
fn test_hurst_mean_reverting_series_below_half() {
    let mut strategy = create_strategy_with_windows(
        InstrumentId::from("PF_XBTUSD.KRAKEN"),
        128,
        vec![4, 8, 16, 32],
    );

    for i in 0..strategy.config.hurst_window {
        strategy
            .returns
            .push_back(if i % 2 == 0 { 0.01 } else { -0.01 });
    }

    let h = strategy.estimate_hurst().unwrap();
    assert!(h < 0.30, "expected mean-reverting Hurst < 0.30, was {h}");
}

#[rstest]
fn test_hurst_persistent_series_above_half() {
    // AR(1) with positive coefficient produces positively autocorrelated returns
    let mut strategy = create_strategy_with_windows(
        InstrumentId::from("PF_XBTUSD.KRAKEN"),
        128,
        vec![4, 8, 16, 32],
    );
    let mut state: u64 = 0x1234_5678_9abc_def0;
    let mut prev = 0.0_f64;

    for _ in 0..strategy.config.hurst_window {
        // Seeded xorshift for deterministic noise
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let u = (state as f64 / u64::MAX as f64 - 0.5) * 0.002;
        let value = 0.9 * prev + u;
        strategy.returns.push_back(value);
        prev = value;
    }

    let h = strategy.estimate_hurst().unwrap();
    assert!(h > 0.70, "expected persistent Hurst > 0.70, was {h}");
}

#[rstest]
fn test_rolling_mean_on_empty_returns_none() {
    let empty: VecDeque<f64> = VecDeque::new();
    assert!(HurstVpinDirectional::rolling_mean(&empty).is_none());
}

#[rstest]
fn test_rolling_mean_basic_average() {
    let mut values: VecDeque<f64> = VecDeque::new();
    values.push_back(1.0);
    values.push_back(2.0);
    values.push_back(3.0);
    assert_eq!(HurstVpinDirectional::rolling_mean(&values), Some(2.0));
}

#[rstest]
fn test_push_bounded_respects_capacity() {
    let mut values: VecDeque<f64> = VecDeque::new();
    HurstVpinDirectional::push_bounded(&mut values, 3, 1.0);
    HurstVpinDirectional::push_bounded(&mut values, 3, 2.0);
    HurstVpinDirectional::push_bounded(&mut values, 3, 3.0);
    HurstVpinDirectional::push_bounded(&mut values, 3, 4.0);

    assert_eq!(values.len(), 3);
    assert_eq!(values[0], 2.0);
    assert_eq!(values[2], 4.0);
}

#[rstest]
fn test_signals_ready_false_during_warmup() {
    let strategy = create_strategy(InstrumentId::from("PF_XBTUSD.KRAKEN"));
    assert!(!strategy.signals_ready());
}

#[rstest]
fn test_on_start_rejects_mismatched_bar_type() {
    let instrument_id = InstrumentId::from("PF_XBTUSD.KRAKEN");
    let other_id = InstrumentId::from("OTHER.KRAKEN");
    let config =
        HurstVpinDirectionalConfig::new(instrument_id, bar_type(other_id), Quantity::from("0.01"));
    let mut strategy = HurstVpinDirectional::new(config);
    register_strategy(&mut strategy);

    let err = strategy.on_start().unwrap_err();
    assert!(
        err.to_string().contains("does not match traded instrument"),
        "unexpected error: {err}"
    );
}

#[rstest]
fn test_on_reset_clears_all_state() {
    let mut strategy = create_strategy(InstrumentId::from("PF_XBTUSD.KRAKEN"));
    register_strategy(&mut strategy);

    strategy.returns.push_back(0.01);
    strategy.abs_imbalances.push_back(0.4);
    strategy.signed_imbalances.push_back(-0.3);
    strategy.last_close = Some(30000.0);
    strategy.bucket_buy_volume = 1.0;
    strategy.bucket_sell_volume = 2.0;
    strategy.hurst = Some(0.6);
    strategy.vpin = Some(0.4);
    strategy.signed_vpin = Some(0.4);
    strategy.position_opened_ns = Some(12_345);
    strategy.exit_cooldown = true;
    strategy.entry_order_id = Some(ClientOrderId::from("O-1"));
    strategy.exit_order_ids.insert(ClientOrderId::from("O-2"));

    strategy.on_reset().unwrap();

    assert!(strategy.returns.is_empty());
    assert!(strategy.abs_imbalances.is_empty());
    assert!(strategy.signed_imbalances.is_empty());
    assert!(strategy.last_close.is_none());
    assert_eq!(strategy.bucket_buy_volume, 0.0);
    assert_eq!(strategy.bucket_sell_volume, 0.0);
    assert!(strategy.hurst.is_none());
    assert!(strategy.vpin.is_none());
    assert!(strategy.signed_vpin.is_none());
    assert!(strategy.position_opened_ns.is_none());
    assert!(!strategy.exit_cooldown);
    assert!(strategy.entry_order_id.is_none());
    assert!(strategy.exit_order_ids.is_empty());
}

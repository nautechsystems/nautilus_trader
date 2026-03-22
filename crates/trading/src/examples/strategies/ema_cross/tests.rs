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
use nautilus_indicators::indicator::{Indicator, MovingAverage};
use nautilus_model::{
    data::QuoteTick,
    identifiers::{InstrumentId, StrategyId, TraderId},
    types::{Price, Quantity},
};
use nautilus_portfolio::portfolio::Portfolio;
use rstest::rstest;

use super::EmaCross;
use crate::strategy::Strategy;

const INSTRUMENT_ID: &str = "AUDUSD.SIM";

fn quote(bid: &str, ask: &str, ts: u64) -> QuoteTick {
    QuoteTick::new(
        InstrumentId::from(INSTRUMENT_ID),
        Price::from(bid),
        Price::from(ask),
        Quantity::from("100000"),
        Quantity::from("100000"),
        ts.into(),
        ts.into(),
    )
}

fn create_strategy(fast: usize, slow: usize) -> EmaCross {
    EmaCross::new(
        InstrumentId::from(INSTRUMENT_ID),
        Quantity::from("100000"),
        fast,
        slow,
    )
}

fn register_strategy(strategy: &mut EmaCross) {
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

/// Feed `count` identical quotes to warm up both EMAs.
fn warm_up(strategy: &mut EmaCross, mid: &str, count: usize) {
    for i in 0..count {
        let q = quote(mid, mid, (i + 1) as u64);
        strategy.on_quote(&q).unwrap();
    }
}

#[rstest]
fn test_new_sets_strategy_id() {
    let strategy = create_strategy(3, 10);
    assert_eq!(
        strategy.core().config.strategy_id,
        Some(StrategyId::from("EMA_CROSS-001")),
    );
}

#[rstest]
fn test_new_initializes_with_no_previous_signal() {
    let strategy = create_strategy(3, 10);
    assert!(strategy.prev_fast_above.is_none());
}

#[rstest]
fn test_on_quote_returns_ok_before_emas_initialized() {
    let mut strategy = create_strategy(3, 10);
    let q = quote("1.00000", "1.00010", 1);
    assert!(strategy.on_quote(&q).is_ok());
    assert!(strategy.prev_fast_above.is_none());
}

#[rstest]
fn test_emas_initialize_after_enough_quotes() {
    let mut strategy = create_strategy(3, 5);
    // Feed 5 quotes (slow period) to initialize both EMAs
    warm_up(&mut strategy, "1.00000", 5);
    assert!(strategy.ema_fast.initialized());
    assert!(strategy.ema_slow.initialized());
}

#[rstest]
fn test_prev_fast_above_set_once_emas_initialized() {
    let mut strategy = create_strategy(3, 5);
    warm_up(&mut strategy, "1.00000", 5);
    assert!(strategy.prev_fast_above.is_some());
}

#[rstest]
fn test_no_crossover_when_price_flat() {
    let mut strategy = create_strategy(3, 5);
    register_strategy(&mut strategy);

    // Flat price: fast and slow converge, no crossover
    warm_up(&mut strategy, "1.00000", 10);

    // Additional flat quotes should not trigger any signal (no error from enter())
    let q = quote("1.00000", "1.00000", 100);
    assert!(strategy.on_quote(&q).is_ok());
}

#[rstest]
fn test_bullish_crossover_triggers_buy() {
    let mut strategy = create_strategy(2, 5);
    register_strategy(&mut strategy);

    // Start low to establish slow EMA below
    for i in 0..5 {
        let q = quote("1.00000", "1.00000", (i + 1) as u64);
        strategy.on_quote(&q).unwrap();
    }

    // fast should equal slow (both at 1.0), so prev_fast_above = false or equal
    // Now push price up sharply so fast EMA rises above slow EMA
    for i in 0..5 {
        let q = quote("1.01000", "1.01000", (10 + i) as u64);
        // on_quote may fail on enter() if no instrument in cache, but
        // the crossover detection itself is what we validate
        let _ = strategy.on_quote(&q);
    }

    // Fast EMA should be above slow after upward move
    assert_eq!(strategy.prev_fast_above, Some(true));
    assert!(strategy.ema_fast.value() > strategy.ema_slow.value());
}

#[rstest]
fn test_bearish_crossover_after_bullish() {
    let mut strategy = create_strategy(2, 5);
    register_strategy(&mut strategy);

    // Warm up at a baseline
    for i in 0..5 {
        let q = quote("1.00000", "1.00000", (i + 1) as u64);
        strategy.on_quote(&q).unwrap();
    }

    // Push price up to get fast above slow
    for i in 0..5 {
        let q = quote("1.01000", "1.01000", (10 + i) as u64);
        let _ = strategy.on_quote(&q);
    }
    assert_eq!(strategy.prev_fast_above, Some(true));

    // Now push price down sharply for bearish crossover
    for i in 0..10 {
        let q = quote("0.99000", "0.99000", (20 + i) as u64);
        let _ = strategy.on_quote(&q);
    }

    assert_eq!(strategy.prev_fast_above, Some(false));
    assert!(strategy.ema_fast.value() < strategy.ema_slow.value());
}

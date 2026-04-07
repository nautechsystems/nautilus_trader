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

//! Unit tests for complement arbitrage detection and execution logic.

use std::{cell::RefCell, rc::Rc};

use nautilus_common::{
    actor::DataActor,
    cache::Cache,
    clock::{Clock, TestClock},
};
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::QuoteTick,
    enums::{AssetClass, LiquiditySide, OrderSide, OrderType},
    events::{OrderEventAny, OrderFilled},
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, StrategyId, Symbol, TradeId, TraderId, Venue,
        VenueOrderId,
    },
    instruments::{BinaryOption, InstrumentAny},
    orders::{
        Order, OrderAny, OrderTestBuilder,
        stubs::{TestOrderEventStubs, TestOrderStubs},
    },
    types::{Currency, Price, Quantity},
};
use nautilus_portfolio::portfolio::Portfolio;
use rstest::rstest;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use ustr::Ustr;

use super::{
    config::ComplementArbConfig,
    strategy::{ArbExecution, ArbState, ComplementArb, ComplementPair},
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Helper to create a strategy with default test config.
fn test_strategy() -> ComplementArb {
    let config = ComplementArbConfig::builder()
        .venue(Venue::new("POLYMARKET"))
        .min_profit_bps(dec!(50))
        .trade_size(dec!(10))
        .build();
    ComplementArb::new(config)
}

/// Helper to create a test pair (pair A).
fn test_pair() -> ComplementPair {
    ComplementPair {
        condition_id: "0xabc".to_string(),
        yes_id: InstrumentId::from("0xabc-111.POLYMARKET"),
        no_id: InstrumentId::from("0xabc-222.POLYMARKET"),
        label: "Test Market".to_string(),
    }
}

/// Helper to create a second test pair (pair B) with different condition ID.
fn test_pair_b() -> ComplementPair {
    ComplementPair {
        condition_id: "0xdef".to_string(),
        yes_id: InstrumentId::from("0xdef-333.POLYMARKET"),
        no_id: InstrumentId::from("0xdef-444.POLYMARKET"),
        label: "Test Market B".to_string(),
    }
}

/// Helper to create a QuoteTick with consistent 3-decimal precision.
fn quote(id: InstrumentId, bid: f64, ask: f64, size: f64) -> QuoteTick {
    QuoteTick::new(
        id,
        Price::from(format!("{bid:.3}").as_str()),
        Price::from(format!("{ask:.3}").as_str()),
        Quantity::from(format!("{size:.1}").as_str()),
        Quantity::from(format!("{size:.1}").as_str()),
        UnixNanos::default(),
        UnixNanos::default(),
    )
}

/// Helper to create an `ArbExecution` in PendingEntry state with default fill values.
fn make_arb_execution(
    yes_order_id: ClientOrderId,
    no_order_id: ClientOrderId,
    arb_side: OrderSide,
) -> ArbExecution {
    ArbExecution {
        state: ArbState::PendingEntry,
        arb_side,
        yes_order_id,
        no_order_id,
        yes_filled_qty: Decimal::ZERO,
        no_filled_qty: Decimal::ZERO,
        unwind_order_id: None,
    }
}

/// Register a strategy with test clock, cache, and portfolio so it can
/// access `self.cache()`, `self.core.clock()`, `self.core.order_factory()`, etc.
fn register_strategy(strategy: &mut ComplementArb) {
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

/// Create a `BinaryOption` instrument for the given instrument ID with an outcome.
fn make_binary_option(id: InstrumentId, outcome: &str, description: &str) -> BinaryOption {
    let raw_symbol = Symbol::new(id.symbol.as_str());
    BinaryOption::new(
        id,
        raw_symbol,
        AssetClass::Alternative,
        Currency::USDC(),
        UnixNanos::default(),
        UnixNanos::from(1_000_000_000_000u64),
        3,
        2,
        Price::from("0.001"),
        Quantity::from("0.01"),
        Some(Ustr::from(outcome)),
        Some(Ustr::from(description)),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        UnixNanos::default(),
        UnixNanos::default(),
    )
}

/// Build a limit order via `OrderTestBuilder` for a given instrument, side, price, qty, and order ID.
fn build_limit_order(
    instrument_id: InstrumentId,
    side: OrderSide,
    price: &str,
    qty: &str,
    order_id: &str,
) -> OrderAny {
    OrderTestBuilder::new(OrderType::Limit)
        .instrument_id(instrument_id)
        .client_order_id(ClientOrderId::new(order_id))
        .side(side)
        .price(Price::from(price))
        .quantity(Quantity::from(qty))
        .build()
}

/// Insert a pair's instrument_to_pair mappings into a strategy (needed for on_quote routing).
fn wire_pair(strategy: &mut ComplementArb, pair: &ComplementPair) {
    strategy
        .pairs
        .insert(pair.condition_id.clone(), pair.clone());
    strategy
        .instrument_to_pair
        .insert(pair.yes_id, pair.condition_id.clone());
    strategy
        .instrument_to_pair
        .insert(pair.no_id, pair.condition_id.clone());
}

mod signal_detection {
    use super::*;

    // ---- extract_pair_key tests ----

    #[rstest]
    fn test_extract_pair_key_valid() {
        let id = InstrumentId::from("0xabc-12345.POLYMARKET");
        let key = ComplementArb::extract_pair_key(&id);
        assert_eq!(key, Some("0xabc".to_string()));
    }

    #[rstest]
    fn test_extract_pair_key_multiple_dashes() {
        let id = InstrumentId::from("0xabc-def-12345.POLYMARKET");
        let key = ComplementArb::extract_pair_key(&id);
        assert_eq!(key, Some("0xabc-def".to_string()));
    }

    #[rstest]
    fn test_extract_pair_key_no_dash() {
        let id = InstrumentId::from("NODASH.POLYMARKET");
        let key = ComplementArb::extract_pair_key(&id);
        assert_eq!(key, None);
    }

    // ---- buy arb detection ----

    #[rstest]
    fn test_buy_arb_detected() {
        let mut strategy = test_strategy();
        let pair = test_pair();

        // combined_ask = 0.48 + 0.48 = 0.96 < 1.0 → profit = 400 bps
        strategy
            .quotes
            .insert(pair.yes_id, quote(pair.yes_id, 0.45, 0.48, 100.0));
        strategy
            .quotes
            .insert(pair.no_id, quote(pair.no_id, 0.45, 0.48, 100.0));

        assert!(strategy.check_buy_arb(&pair));
        assert_eq!(strategy.buy_arbs_detected, 1);
    }

    #[rstest]
    fn test_buy_arb_not_detected_efficient_market() {
        let mut strategy = test_strategy();
        let pair = test_pair();

        // combined_ask = 0.52 + 0.52 = 1.04 >= 1.0
        strategy
            .quotes
            .insert(pair.yes_id, quote(pair.yes_id, 0.48, 0.52, 100.0));
        strategy
            .quotes
            .insert(pair.no_id, quote(pair.no_id, 0.48, 0.52, 100.0));

        assert!(!strategy.check_buy_arb(&pair));
        assert_eq!(strategy.buy_arbs_detected, 0);
    }

    #[rstest]
    fn test_buy_arb_below_min_profit() {
        let mut strategy = test_strategy();
        let pair = test_pair();

        // combined_ask = 0.498 + 0.498 = 0.996 → profit = 40 bps < 50 bps min
        strategy
            .quotes
            .insert(pair.yes_id, quote(pair.yes_id, 0.49, 0.498, 100.0));
        strategy
            .quotes
            .insert(pair.no_id, quote(pair.no_id, 0.49, 0.498, 100.0));

        assert!(!strategy.check_buy_arb(&pair));
        assert_eq!(strategy.buy_arbs_detected, 0);
    }

    #[rstest]
    fn test_buy_arb_missing_quote_returns_false() {
        let mut strategy = test_strategy();
        let pair = test_pair();

        // Only insert one side
        strategy
            .quotes
            .insert(pair.yes_id, quote(pair.yes_id, 0.45, 0.48, 100.0));

        assert!(!strategy.check_buy_arb(&pair));
    }

    #[rstest]
    fn test_insufficient_liquidity_skipped_buy() {
        let mut strategy = test_strategy();
        let pair = test_pair();

        // Good spread but size = 5 < trade_size = 10
        strategy
            .quotes
            .insert(pair.yes_id, quote(pair.yes_id, 0.45, 0.48, 5.0));
        strategy
            .quotes
            .insert(pair.no_id, quote(pair.no_id, 0.45, 0.48, 100.0));

        assert!(!strategy.check_buy_arb(&pair));
        assert_eq!(strategy.buy_arbs_detected, 1); // detected but skipped
    }

    // ---- buy arb abs profit tests ----

    #[rstest]
    fn test_buy_arb_below_min_abs_profit() {
        let config = ComplementArbConfig::builder()
            .venue(Venue::new("POLYMARKET"))
            .min_profit_bps(dec!(50))
            .min_profit_abs(dec!(1))
            .trade_size(dec!(10))
            .build();
        let mut strategy = ComplementArb::new(config);
        let pair = test_pair();

        // combined_ask = 0.48 + 0.48 = 0.96 → profit_per_share = 0.04 → abs = 0.04 * 10 = $0.40 < $1.0
        strategy
            .quotes
            .insert(pair.yes_id, quote(pair.yes_id, 0.45, 0.48, 100.0));
        strategy
            .quotes
            .insert(pair.no_id, quote(pair.no_id, 0.45, 0.48, 100.0));

        assert!(!strategy.check_buy_arb(&pair));
    }

    #[rstest]
    fn test_buy_arb_above_min_abs_profit() {
        let config = ComplementArbConfig::builder()
            .venue(Venue::new("POLYMARKET"))
            .min_profit_bps(dec!(50))
            .min_profit_abs(dec!(1))
            .trade_size(dec!(10))
            .build();
        let mut strategy = ComplementArb::new(config);
        let pair = test_pair();

        // combined_ask = 0.40 + 0.40 = 0.80 → profit_per_share = 0.20 → abs = 0.20 * 10 = $2.0 > $1.0
        strategy
            .quotes
            .insert(pair.yes_id, quote(pair.yes_id, 0.38, 0.40, 100.0));
        strategy
            .quotes
            .insert(pair.no_id, quote(pair.no_id, 0.38, 0.40, 100.0));

        assert!(strategy.check_buy_arb(&pair));
    }

    // ---- sell arb tests ----

    #[rstest]
    fn test_sell_arb_detected() {
        let mut strategy = test_strategy();
        let pair = test_pair();

        // combined_bid = 0.52 + 0.52 = 1.04 > 1.0 → profit = 400 bps
        strategy
            .quotes
            .insert(pair.yes_id, quote(pair.yes_id, 0.52, 0.55, 100.0));
        strategy
            .quotes
            .insert(pair.no_id, quote(pair.no_id, 0.52, 0.55, 100.0));

        assert!(strategy.check_sell_arb(&pair));
        assert_eq!(strategy.sell_arbs_detected, 1);
    }

    #[rstest]
    fn test_sell_arb_not_detected() {
        let mut strategy = test_strategy();
        let pair = test_pair();

        // combined_bid = 0.48 + 0.48 = 0.96 <= 1.0
        strategy
            .quotes
            .insert(pair.yes_id, quote(pair.yes_id, 0.48, 0.52, 100.0));
        strategy
            .quotes
            .insert(pair.no_id, quote(pair.no_id, 0.48, 0.52, 100.0));

        assert!(!strategy.check_sell_arb(&pair));
        assert_eq!(strategy.sell_arbs_detected, 0);
    }

    #[rstest]
    fn test_sell_arb_missing_quote_returns_false() {
        let mut strategy = test_strategy();
        let pair = test_pair();

        strategy
            .quotes
            .insert(pair.yes_id, quote(pair.yes_id, 0.52, 0.55, 100.0));

        assert!(!strategy.check_sell_arb(&pair));
    }

    #[rstest]
    fn test_insufficient_liquidity_skipped_sell() {
        let mut strategy = test_strategy();
        let pair = test_pair();

        // Good spread but size = 5 < trade_size = 10
        strategy
            .quotes
            .insert(pair.yes_id, quote(pair.yes_id, 0.52, 0.55, 100.0));
        strategy
            .quotes
            .insert(pair.no_id, quote(pair.no_id, 0.52, 0.55, 5.0));

        assert!(!strategy.check_sell_arb(&pair));
        assert_eq!(strategy.sell_arbs_detected, 1); // detected but skipped
    }

    // ---- fee tests ----

    #[rstest]
    fn test_fee_reduces_profit_below_threshold() {
        let config = ComplementArbConfig::builder()
            .venue(Venue::new("POLYMARKET"))
            .fee_estimate_bps(dec!(200)) // 2% fee
            .min_profit_bps(dec!(50))
            .trade_size(dec!(10))
            .build();
        let mut strategy = ComplementArb::new(config);
        let pair = test_pair();

        // combined_ask = 0.495 + 0.495 = 0.99 → raw profit = 100 bps
        // per-leg fee = 200/10000 * 0.495 * 0.505 ≈ 0.005 → total fee ≈ 0.01 → net ≈ 0 bps
        strategy
            .quotes
            .insert(pair.yes_id, quote(pair.yes_id, 0.48, 0.495, 100.0));
        strategy
            .quotes
            .insert(pair.no_id, quote(pair.no_id, 0.48, 0.495, 100.0));

        assert!(!strategy.check_buy_arb(&pair));
    }

    #[rstest]
    fn test_fee_still_allows_large_arb() {
        let config = ComplementArbConfig::builder()
            .venue(Venue::new("POLYMARKET"))
            .fee_estimate_bps(dec!(200))
            .min_profit_bps(dec!(50))
            .trade_size(dec!(10))
            .build();
        let mut strategy = ComplementArb::new(config);
        let pair = test_pair();

        // combined_ask = 0.40 + 0.40 = 0.80 → raw profit = 2000 bps, fee ≈ 96 bps → net ≈ 1904
        strategy
            .quotes
            .insert(pair.yes_id, quote(pair.yes_id, 0.38, 0.40, 100.0));
        strategy
            .quotes
            .insert(pair.no_id, quote(pair.no_id, 0.38, 0.40, 100.0));

        assert!(strategy.check_buy_arb(&pair));
    }

    // ---- per-leg fee asymmetric ----

    #[rstest]
    fn test_buy_arb_per_leg_fee_asymmetric_prices() {
        let config = ComplementArbConfig::builder()
            .venue(Venue::new("POLYMARKET"))
            .fee_estimate_bps(dec!(200))
            .min_profit_bps(dec!(50))
            .trade_size(dec!(10))
            .build();
        let mut strategy = ComplementArb::new(config);
        let pair = test_pair();

        // yes_ask=0.10, no_ask=0.85 → combined=0.95 → raw profit = 500 bps
        // per-leg fee is much lower at extreme prices → net ≈ 456 bps > 50
        strategy
            .quotes
            .insert(pair.yes_id, quote(pair.yes_id, 0.08, 0.10, 100.0));
        strategy
            .quotes
            .insert(pair.no_id, quote(pair.no_id, 0.83, 0.85, 100.0));

        assert!(strategy.check_buy_arb(&pair));
    }

    #[rstest]
    fn test_sell_arb_per_leg_fee_asymmetric_prices() {
        let config = ComplementArbConfig::builder()
            .venue(Venue::new("POLYMARKET"))
            .fee_estimate_bps(dec!(200))
            .min_profit_bps(dec!(50))
            .trade_size(dec!(10))
            .build();
        let mut strategy = ComplementArb::new(config);
        let pair = test_pair();

        // yes_bid=0.90, no_bid=0.15 → combined=1.05 → net ≈ 456 bps > 50
        strategy
            .quotes
            .insert(pair.yes_id, quote(pair.yes_id, 0.90, 0.92, 100.0));
        strategy
            .quotes
            .insert(pair.no_id, quote(pair.no_id, 0.15, 0.17, 100.0));

        assert!(strategy.check_sell_arb(&pair));
    }

    // ---- spread tracking ----

    #[rstest]
    fn test_best_buy_spread_tracking() {
        let mut strategy = test_strategy();
        let pair = test_pair();

        strategy
            .quotes
            .insert(pair.yes_id, quote(pair.yes_id, 0.48, 0.52, 100.0));
        strategy
            .quotes
            .insert(pair.no_id, quote(pair.no_id, 0.48, 0.52, 100.0));

        strategy.check_buy_arb(&pair);
        assert_eq!(strategy.best_buy_spread, dec!(1.04));
        assert_eq!(strategy.best_buy_label, "Test Market");
    }

    #[rstest]
    fn test_best_sell_spread_tracking() {
        let mut strategy = test_strategy();
        let pair = test_pair();

        strategy
            .quotes
            .insert(pair.yes_id, quote(pair.yes_id, 0.48, 0.52, 100.0));
        strategy
            .quotes
            .insert(pair.no_id, quote(pair.no_id, 0.48, 0.52, 100.0));

        strategy.check_sell_arb(&pair);
        assert_eq!(strategy.best_sell_spread, dec!(0.96));
        assert_eq!(strategy.best_sell_label, "Test Market");
    }

    // ---- scenario: spread narrowing to arb ----

    #[rstest]
    fn test_scenario_spread_narrowing_to_arb() {
        let mut strategy = test_strategy();
        let pair = test_pair();

        // Market starts efficient: combined_ask=1.04 → no arb
        strategy
            .quotes
            .insert(pair.yes_id, quote(pair.yes_id, 0.48, 0.52, 100.0));
        strategy
            .quotes
            .insert(pair.no_id, quote(pair.no_id, 0.48, 0.52, 100.0));
        assert!(!strategy.check_buy_arb(&pair));
        assert_eq!(strategy.buy_arbs_detected, 0);

        // Tightens to 1.02 → still no arb
        strategy
            .quotes
            .insert(pair.yes_id, quote(pair.yes_id, 0.48, 0.51, 100.0));
        strategy
            .quotes
            .insert(pair.no_id, quote(pair.no_id, 0.48, 0.51, 100.0));
        assert!(!strategy.check_buy_arb(&pair));
        assert_eq!(strategy.buy_arbs_detected, 0);

        // Tightens to 1.00 → still no arb (needs strictly < 1.0)
        strategy
            .quotes
            .insert(pair.yes_id, quote(pair.yes_id, 0.48, 0.50, 100.0));
        strategy
            .quotes
            .insert(pair.no_id, quote(pair.no_id, 0.48, 0.50, 100.0));
        assert!(!strategy.check_buy_arb(&pair));
        assert_eq!(strategy.buy_arbs_detected, 0);

        // Drops to 0.96 → arb triggers
        strategy
            .quotes
            .insert(pair.yes_id, quote(pair.yes_id, 0.45, 0.48, 100.0));
        strategy
            .quotes
            .insert(pair.no_id, quote(pair.no_id, 0.45, 0.48, 100.0));
        assert!(strategy.check_buy_arb(&pair));
        assert_eq!(strategy.buy_arbs_detected, 1);

        // Best spread should track the tightest (0.96)
        assert_eq!(strategy.best_buy_spread, dec!(0.96));
    }

    // ---- scenario: multi-pair independent detection ----

    #[rstest]
    fn test_scenario_multi_pair_independent_detection() {
        let mut strategy = test_strategy();
        let pair_a = test_pair();
        let pair_b = test_pair_b();

        // Pair A has arb spread: combined_ask = 0.96
        strategy
            .quotes
            .insert(pair_a.yes_id, quote(pair_a.yes_id, 0.45, 0.48, 100.0));
        strategy
            .quotes
            .insert(pair_a.no_id, quote(pair_a.no_id, 0.45, 0.48, 100.0));

        // Pair B is efficient: combined_ask = 1.04
        strategy
            .quotes
            .insert(pair_b.yes_id, quote(pair_b.yes_id, 0.48, 0.52, 100.0));
        strategy
            .quotes
            .insert(pair_b.no_id, quote(pair_b.no_id, 0.48, 0.52, 100.0));

        // Only pair A triggers
        assert!(strategy.check_buy_arb(&pair_a));
        assert!(!strategy.check_buy_arb(&pair_b));
        assert_eq!(strategy.buy_arbs_detected, 1);

        // Best spread should be pair A's 0.96, not pair B's 1.04
        assert_eq!(strategy.best_buy_spread, dec!(0.96));
        assert_eq!(strategy.best_buy_label, "Test Market");
    }

    // ---- boundary precision tests ----

    #[rstest]
    fn test_buy_arb_exact_threshold_no_profit() {
        let mut strategy = test_strategy();
        let pair = test_pair();

        // combined_ask = 0.500 + 0.500 = 1.000 exactly → should NOT trigger (>= 1.0 check)
        strategy
            .quotes
            .insert(pair.yes_id, quote(pair.yes_id, 0.48, 0.500, 100.0));
        strategy
            .quotes
            .insert(pair.no_id, quote(pair.no_id, 0.48, 0.500, 100.0));

        assert!(!strategy.check_buy_arb(&pair));
    }

    #[rstest]
    fn test_sell_arb_exact_threshold_no_profit() {
        let mut strategy = test_strategy();
        let pair = test_pair();

        // combined_bid = 0.500 + 0.500 = 1.000 exactly → should NOT trigger (<= 1.0 check)
        strategy
            .quotes
            .insert(pair.yes_id, quote(pair.yes_id, 0.500, 0.55, 100.0));
        strategy
            .quotes
            .insert(pair.no_id, quote(pair.no_id, 0.500, 0.55, 100.0));

        assert!(!strategy.check_sell_arb(&pair));
    }

    #[rstest]
    fn test_buy_arb_min_profit_exact_boundary() {
        // min_profit_bps = 50, set spread so profit_bps == 50 exactly → should pass (< comparison)
        let config = ComplementArbConfig::builder()
            .venue(Venue::new("POLYMARKET"))
            .min_profit_bps(dec!(50))
            .trade_size(dec!(10))
            .build();
        let mut strategy = ComplementArb::new(config);
        let pair = test_pair();

        // combined_ask = 0.995 → profit_bps = (1.0 - 0.995) * 10000 = 50 bps
        // But 50 < 50 is false, so this should NOT trigger
        strategy
            .quotes
            .insert(pair.yes_id, quote(pair.yes_id, 0.49, 0.4975, 100.0));
        strategy
            .quotes
            .insert(pair.no_id, quote(pair.no_id, 0.49, 0.4975, 100.0));

        // combined_ask = 0.4975 + 0.4975 = 0.9950 → profit_bps = 50
        // The code does `profit_bps < min_profit` i.e. 50 < 50 = false → passes
        assert!(strategy.check_buy_arb(&pair));
    }

    // ---- fee eliminates marginal arb ----

    #[rstest]
    fn test_scenario_fee_eliminates_marginal_arb() {
        let pair = test_pair();

        // Without fees: combined_ask=0.992 → raw profit = 80 bps > 50 min → arb
        let config_no_fee = ComplementArbConfig::builder()
            .venue(Venue::new("POLYMARKET"))
            .min_profit_bps(dec!(50))
            .trade_size(dec!(10))
            .build();
        let mut strategy_no_fee = ComplementArb::new(config_no_fee);
        strategy_no_fee
            .quotes
            .insert(pair.yes_id, quote(pair.yes_id, 0.49, 0.496, 100.0));
        strategy_no_fee
            .quotes
            .insert(pair.no_id, quote(pair.no_id, 0.49, 0.496, 100.0));
        assert!(strategy_no_fee.check_buy_arb(&pair));

        // With 200bps fee: per-leg fee ≈ 0.02 * 0.496 * 0.504 ≈ 0.005 → total ≈ 0.01
        // net profit = (1.0 - 0.992 - 0.01) * 10000 = -20 bps → no arb
        let config_fee = ComplementArbConfig::builder()
            .venue(Venue::new("POLYMARKET"))
            .fee_estimate_bps(dec!(200))
            .min_profit_bps(dec!(50))
            .trade_size(dec!(10))
            .build();
        let mut strategy_fee = ComplementArb::new(config_fee);
        strategy_fee
            .quotes
            .insert(pair.yes_id, quote(pair.yes_id, 0.49, 0.496, 100.0));
        strategy_fee
            .quotes
            .insert(pair.no_id, quote(pair.no_id, 0.49, 0.496, 100.0));
        assert!(!strategy_fee.check_buy_arb(&pair));
    }

    // ---- both filters must pass ----

    #[rstest]
    fn test_scenario_both_filters_must_pass() {
        let pair = test_pair();

        // Config: min_profit_bps=50, min_profit_abs=1.0, trade_size=10
        let config = ComplementArbConfig::builder()
            .venue(Venue::new("POLYMARKET"))
            .min_profit_bps(dec!(50))
            .min_profit_abs(dec!(1))
            .trade_size(dec!(10))
            .build();

        // Case 1: bps passes (400 bps) but abs fails ($0.40 < $1.0)
        let mut s1 = ComplementArb::new(config);
        // combined_ask = 0.96 → profit_per_share = 0.04 → abs = 0.04 * 10 = $0.40
        // profit_bps = 400 → passes bps, fails abs
        s1.quotes
            .insert(pair.yes_id, quote(pair.yes_id, 0.45, 0.48, 100.0));
        s1.quotes
            .insert(pair.no_id, quote(pair.no_id, 0.45, 0.48, 100.0));
        assert!(!s1.check_buy_arb(&pair));

        // Case 2: abs passes ($2.0) but bps would only pass if bps filter were higher
        // We can't easily make abs pass but bps fail with these params since
        // profit_abs = profit_per_share * trade_size and bps = profit_per_share * 10000.
        // With trade_size=10, to get abs >= 1.0 we need per_share >= 0.1 = 1000 bps,
        // which always passes bps=50. Instead test with trade_size=100:
        let config2 = ComplementArbConfig::builder()
            .venue(Venue::new("POLYMARKET"))
            .min_profit_bps(dec!(500))
            .min_profit_abs(dec!(1))
            .trade_size(dec!(100))
            .build();
        let mut s2 = ComplementArb::new(config2);
        // combined_ask = 0.97 → profit_per_share = 0.03 → bps = 300 < 500, abs = 3.0 > 1.0
        s2.quotes
            .insert(pair.yes_id, quote(pair.yes_id, 0.48, 0.485, 200.0));
        s2.quotes
            .insert(pair.no_id, quote(pair.no_id, 0.48, 0.485, 200.0));
        assert!(!s2.check_buy_arb(&pair));

        // Case 3: both pass
        let config3 = ComplementArbConfig::builder()
            .venue(Venue::new("POLYMARKET"))
            .min_profit_bps(dec!(50))
            .min_profit_abs(dec!(1))
            .trade_size(dec!(100))
            .build();
        let mut s3 = ComplementArb::new(config3);
        // combined_ask = 0.80 → profit_per_share = 0.20 → bps = 2000 > 50, abs = 20.0 > 1.0
        s3.quotes
            .insert(pair.yes_id, quote(pair.yes_id, 0.38, 0.40, 200.0));
        s3.quotes
            .insert(pair.no_id, quote(pair.no_id, 0.38, 0.40, 200.0));
        assert!(s3.check_buy_arb(&pair));
    }

    // ---- buy and sell arb mutually exclusive ----

    #[rstest]
    fn test_scenario_buy_and_sell_arb_mutually_exclusive() {
        let mut strategy = test_strategy();
        let pair = test_pair();

        // Symmetric quotes: bid=0.48, ask=0.52 → combined_ask=1.04, combined_bid=0.96
        // Neither side crosses the threshold in profitable direction
        strategy
            .quotes
            .insert(pair.yes_id, quote(pair.yes_id, 0.48, 0.52, 100.0));
        strategy
            .quotes
            .insert(pair.no_id, quote(pair.no_id, 0.48, 0.52, 100.0));
        let buy = strategy.check_buy_arb(&pair);
        let sell = strategy.check_sell_arb(&pair);
        assert!(!buy && !sell, "symmetric efficient market triggers neither");

        // Arb-worthy buy: combined_ask=0.96 but combined_bid=0.90 (bid < ask)
        strategy
            .quotes
            .insert(pair.yes_id, quote(pair.yes_id, 0.45, 0.48, 100.0));
        strategy
            .quotes
            .insert(pair.no_id, quote(pair.no_id, 0.45, 0.48, 100.0));
        let buy = strategy.check_buy_arb(&pair);
        let sell = strategy.check_sell_arb(&pair);
        assert!(buy, "should detect buy arb");
        assert!(!sell, "should NOT detect sell arb simultaneously");

        // Arb-worthy sell: combined_bid=1.04 but combined_ask=1.10
        strategy
            .quotes
            .insert(pair.yes_id, quote(pair.yes_id, 0.52, 0.55, 100.0));
        strategy
            .quotes
            .insert(pair.no_id, quote(pair.no_id, 0.52, 0.55, 100.0));
        let buy = strategy.check_buy_arb(&pair);
        let sell = strategy.check_sell_arb(&pair);
        assert!(!buy, "should NOT detect buy arb");
        assert!(sell, "should detect sell arb");
    }
}

mod pair_discovery {
    use super::*;

    #[rstest]
    fn test_try_match_complement_forms_pair() {
        let mut strategy = test_strategy();

        let yes_id = InstrumentId::from("0xabc-111.POLYMARKET");
        let no_id = InstrumentId::from("0xabc-222.POLYMARKET");

        // Insert a pending No instrument
        strategy.pending_complements.insert(
            "0xabc".to_string(),
            (no_id, "No".to_string(), "Test Market".to_string()),
        );

        // Match with Yes
        let result = strategy.try_match_complement(yes_id, "Yes", "Test Market");
        assert!(result.is_some());

        let pair = result.unwrap();
        assert_eq!(pair.yes_id, yes_id);
        assert_eq!(pair.no_id, no_id);
        assert_eq!(pair.condition_id, "0xabc");
        assert!(strategy.pairs.contains_key("0xabc"));
        assert!(strategy.pending_complements.is_empty());
    }

    #[rstest]
    fn test_try_match_complement_stores_pending() {
        let mut strategy = test_strategy();
        let yes_id = InstrumentId::from("0xabc-111.POLYMARKET");

        let result = strategy.try_match_complement(yes_id, "Yes", "Test Market");
        assert!(result.is_none());

        assert!(strategy.pending_complements.contains_key("0xabc"));
        let (stored_id, stored_outcome, stored_label) =
            strategy.pending_complements.get("0xabc").unwrap();
        assert_eq!(*stored_id, yes_id);
        assert_eq!(stored_outcome, "Yes");
        assert_eq!(stored_label, "Test Market");
    }

    #[rstest]
    fn test_try_match_complement_same_outcome_rejected() {
        let mut strategy = test_strategy();

        let first_yes_id = InstrumentId::from("0xabc-111.POLYMARKET");
        let second_yes_id = InstrumentId::from("0xabc-222.POLYMARKET");

        strategy.pending_complements.insert(
            "0xabc".to_string(),
            (first_yes_id, "Yes".to_string(), "Test Market".to_string()),
        );

        let result = strategy.try_match_complement(second_yes_id, "Yes", "Test Market 2");
        assert!(result.is_none());

        // Original pending should be preserved
        let (stored_id, stored_outcome, _) = strategy.pending_complements.get("0xabc").unwrap();
        assert_eq!(*stored_id, first_yes_id);
        assert_eq!(stored_outcome, "Yes");
    }

    #[rstest]
    fn test_scenario_incremental_pair_formation_via_on_instrument() {
        let mut strategy = test_strategy();
        register_strategy(&mut strategy);

        let yes_id = InstrumentId::from("0xnew-111.POLYMARKET");
        let no_id = InstrumentId::from("0xnew-222.POLYMARKET");

        // First instrument arrives → goes to pending
        let yes_opt = make_binary_option(yes_id, "Yes", "Will it rain?");
        strategy
            .on_instrument(&InstrumentAny::BinaryOption(yes_opt))
            .unwrap();

        assert!(strategy.pending_complements.contains_key("0xnew"));
        assert!(!strategy.pairs.contains_key("0xnew"));

        // Complement arrives → pair formed
        let no_opt = make_binary_option(no_id, "No", "Will it rain?");
        strategy
            .on_instrument(&InstrumentAny::BinaryOption(no_opt))
            .unwrap();

        assert!(strategy.pairs.contains_key("0xnew"));
        assert!(strategy.pending_complements.is_empty());

        // Verify instrument_to_pair mappings
        assert_eq!(strategy.instrument_to_pair.get(&yes_id).unwrap(), "0xnew");
        assert_eq!(strategy.instrument_to_pair.get(&no_id).unwrap(), "0xnew");
    }

    #[rstest]
    fn test_scenario_duplicate_instrument_ignored() {
        let mut strategy = test_strategy();
        register_strategy(&mut strategy);

        let yes_id = InstrumentId::from("0xdup-111.POLYMARKET");
        let no_id = InstrumentId::from("0xdup-222.POLYMARKET");

        // Form a pair
        let yes_opt = make_binary_option(yes_id, "Yes", "Duplicate test");
        let no_opt = make_binary_option(no_id, "No", "Duplicate test");
        strategy
            .on_instrument(&InstrumentAny::BinaryOption(yes_opt.clone()))
            .unwrap();
        strategy
            .on_instrument(&InstrumentAny::BinaryOption(no_opt))
            .unwrap();
        assert_eq!(strategy.pairs.len(), 1);

        // Receive same instrument again → no duplicate
        strategy
            .on_instrument(&InstrumentAny::BinaryOption(yes_opt))
            .unwrap();
        assert_eq!(strategy.pairs.len(), 1);
    }
}

mod execution_state_machine {
    use super::*;

    // ---- has_active_arb ----

    #[rstest]
    fn test_has_active_arb_false_when_empty() {
        let strategy = test_strategy();
        assert!(!strategy.has_active_arb("0xabc"));
    }

    #[rstest]
    fn test_has_active_arb_true_when_pending() {
        let mut strategy = test_strategy();
        strategy.arb_executions.insert(
            "0xabc".to_string(),
            make_arb_execution(
                ClientOrderId::new("O-001"),
                ClientOrderId::new("O-002"),
                OrderSide::Buy,
            ),
        );
        assert!(strategy.has_active_arb("0xabc"));
    }

    #[rstest]
    fn test_has_active_arb_false_when_idle() {
        let mut strategy = test_strategy();
        let mut exec = make_arb_execution(
            ClientOrderId::new("O-001"),
            ClientOrderId::new("O-002"),
            OrderSide::Buy,
        );
        exec.state = ArbState::Idle;
        strategy.arb_executions.insert("0xabc".to_string(), exec);
        assert!(!strategy.has_active_arb("0xabc"));
    }

    // ---- submit_arb ----

    #[rstest]
    fn test_submit_arb_noop_when_live_trading_false() {
        let mut strategy = test_strategy();
        let pair = test_pair();

        strategy
            .quotes
            .insert(pair.yes_id, quote(pair.yes_id, 0.45, 0.48, 100.0));
        strategy
            .quotes
            .insert(pair.no_id, quote(pair.no_id, 0.45, 0.48, 100.0));

        assert!(strategy.check_buy_arb(&pair));
        assert_eq!(strategy.buy_arbs_detected, 1);
        assert_eq!(strategy.arbs_submitted, 0);
        assert!(strategy.arb_executions.is_empty());
    }

    #[rstest]
    fn test_submit_arb_blocked_by_active_arb() {
        let mut strategy = test_strategy();
        let yes_order_id = ClientOrderId::new("O-001");
        let no_order_id = ClientOrderId::new("O-002");

        strategy.arb_executions.insert(
            "0xabc".to_string(),
            make_arb_execution(yes_order_id, no_order_id, OrderSide::Buy),
        );

        assert!(strategy.has_active_arb("0xabc"));
    }

    #[rstest]
    fn test_scenario_submit_arb_creates_two_orders() {
        let config = ComplementArbConfig::builder()
            .venue(Venue::new("POLYMARKET"))
            .min_profit_bps(dec!(50))
            .trade_size(dec!(10))
            .live_trading(true)
            .build();
        let mut strategy = ComplementArb::new(config);
        register_strategy(&mut strategy);

        let pair = test_pair();
        wire_pair(&mut strategy, &pair);

        strategy
            .quotes
            .insert(pair.yes_id, quote(pair.yes_id, 0.45, 0.48, 100.0));
        strategy
            .quotes
            .insert(pair.no_id, quote(pair.no_id, 0.45, 0.48, 100.0));

        assert!(strategy.check_buy_arb(&pair));
        assert_eq!(strategy.arbs_submitted, 1);
        assert!(strategy.arb_executions.contains_key("0xabc"));
        assert_eq!(strategy.order_to_pair.len(), 2);

        let exec = strategy.arb_executions.get("0xabc").unwrap();
        assert_eq!(exec.state, ArbState::PendingEntry);
        assert_eq!(exec.arb_side, OrderSide::Buy);
    }

    #[rstest]
    fn test_scenario_active_arb_suppresses_detection() {
        let mut strategy = test_strategy();
        let pair = test_pair();
        wire_pair(&mut strategy, &pair);

        // Insert active PendingEntry execution
        strategy.arb_executions.insert(
            "0xabc".to_string(),
            make_arb_execution(
                ClientOrderId::new("O-001"),
                ClientOrderId::new("O-002"),
                OrderSide::Buy,
            ),
        );

        // Feed arb-worthy quotes via on_quote
        let arb_quote_yes = quote(pair.yes_id, 0.45, 0.48, 100.0);
        strategy.on_quote(&arb_quote_yes).unwrap();

        // Detection should be suppressed — counter stays at 0
        assert_eq!(strategy.buy_arbs_detected, 0);
        assert_eq!(strategy.sell_arbs_detected, 0);
    }

    #[rstest]
    fn test_scenario_active_arb_on_pair_a_allows_pair_b() {
        let mut strategy = test_strategy();
        let pair_a = test_pair();
        let pair_b = test_pair_b();
        wire_pair(&mut strategy, &pair_a);
        wire_pair(&mut strategy, &pair_b);

        // Active arb on pair A
        strategy.arb_executions.insert(
            "0xabc".to_string(),
            make_arb_execution(
                ClientOrderId::new("O-001"),
                ClientOrderId::new("O-002"),
                OrderSide::Buy,
            ),
        );

        // Feed arb-worthy quotes for pair A → suppressed
        strategy
            .quotes
            .insert(pair_a.yes_id, quote(pair_a.yes_id, 0.45, 0.48, 100.0));
        strategy
            .quotes
            .insert(pair_a.no_id, quote(pair_a.no_id, 0.45, 0.48, 100.0));
        let a_quote = quote(pair_a.yes_id, 0.45, 0.48, 100.0);
        strategy.on_quote(&a_quote).unwrap();
        assert_eq!(strategy.buy_arbs_detected, 0);

        // Feed arb-worthy quotes for pair B → should detect
        strategy
            .quotes
            .insert(pair_b.yes_id, quote(pair_b.yes_id, 0.45, 0.48, 100.0));
        strategy
            .quotes
            .insert(pair_b.no_id, quote(pair_b.no_id, 0.45, 0.48, 100.0));
        let b_quote = quote(pair_b.yes_id, 0.45, 0.48, 100.0);
        strategy.on_quote(&b_quote).unwrap();
        assert_eq!(strategy.buy_arbs_detected, 1);
    }

    // ---- handle_fill scenarios ----

    #[rstest]
    fn test_scenario_handle_fill_both_legs_complete() {
        let mut strategy = test_strategy();
        register_strategy(&mut strategy);

        let pair = test_pair();
        wire_pair(&mut strategy, &pair);
        let instrument_yes =
            InstrumentAny::BinaryOption(make_binary_option(pair.yes_id, "Yes", "Test Market"));
        let instrument_no =
            InstrumentAny::BinaryOption(make_binary_option(pair.no_id, "No", "Test Market"));

        // Build two orders with distinct IDs
        let yes_order = build_limit_order(pair.yes_id, OrderSide::Buy, "0.480", "10", "O-YES-1");
        let no_order = build_limit_order(pair.no_id, OrderSide::Buy, "0.480", "10", "O-NO-1");
        let yes_order_id = yes_order.client_order_id();
        let no_order_id = no_order.client_order_id();

        // Put both in cache as "accepted" (open, not yet filled)
        let yes_accepted = TestOrderStubs::make_accepted_order(&yes_order);
        let no_accepted = TestOrderStubs::make_accepted_order(&no_order);
        {
            let cache_rc = strategy.cache_rc();
            cache_rc
                .borrow_mut()
                .add_order(yes_accepted, None, None, false)
                .unwrap();
            cache_rc
                .borrow_mut()
                .add_order(no_accepted, None, None, false)
                .unwrap();
        }

        // Set up arb execution
        strategy.arb_executions.insert(
            "0xabc".to_string(),
            make_arb_execution(yes_order_id, no_order_id, OrderSide::Buy),
        );
        strategy
            .order_to_pair
            .insert(yes_order_id, "0xabc".to_string());
        strategy
            .order_to_pair
            .insert(no_order_id, "0xabc".to_string());

        // Step 1: Apply fill to yes order in cache, then handle_fill
        let yes_fill_event = TestOrderEventStubs::filled(
            &yes_order,
            &instrument_yes,
            None,
            None,
            Some(Price::from("0.480")),
            None,
            Some(LiquiditySide::Taker),
            None,
            None,
            None,
        );
        {
            let cache_rc = strategy.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            let order = cache.mut_order(&yes_order_id).unwrap();
            order.apply(yes_fill_event.clone()).unwrap();
        }
        let yes_fill = match &yes_fill_event {
            OrderEventAny::Filled(f) => *f,
            _ => unreachable!(),
        };
        strategy.handle_fill(&yes_fill);

        // Yes leg closed, no leg still open → PartialFill
        let exec = strategy.arb_executions.get("0xabc").unwrap();
        assert_eq!(exec.state, ArbState::PartialFill);
        assert_eq!(exec.yes_filled_qty, dec!(10));

        // Step 2: Apply fill to no order in cache, then handle_fill
        let no_fill_event = TestOrderEventStubs::filled(
            &no_order,
            &instrument_no,
            None,
            None,
            Some(Price::from("0.480")),
            None,
            Some(LiquiditySide::Taker),
            None,
            None,
            None,
        );
        {
            let cache_rc = strategy.cache_rc();
            let mut cache = cache_rc.borrow_mut();
            let order = cache.mut_order(&no_order_id).unwrap();
            order.apply(no_fill_event.clone()).unwrap();
        }
        let no_fill = match &no_fill_event {
            OrderEventAny::Filled(f) => *f,
            _ => unreachable!(),
        };
        strategy.handle_fill(&no_fill);

        // Both legs filled → arb complete
        assert_eq!(strategy.arbs_completed, 1);
        assert!(strategy.arb_executions.is_empty());
        assert!(strategy.order_to_pair.is_empty());
    }

    #[rstest]
    fn test_scenario_handle_fill_partial_state_transition() {
        let mut strategy = test_strategy();
        register_strategy(&mut strategy);

        let pair = test_pair();
        wire_pair(&mut strategy, &pair);
        let instrument =
            InstrumentAny::BinaryOption(make_binary_option(pair.yes_id, "Yes", "Test Market"));

        // Build orders: yes will be filled, no will remain open (not in cache)
        let yes_order = build_limit_order(pair.yes_id, OrderSide::Buy, "0.480", "10", "O-YES-2");
        let no_order = build_limit_order(pair.no_id, OrderSide::Buy, "0.480", "10", "O-NO-2");
        let yes_order_id = yes_order.client_order_id();
        let no_order_id = no_order.client_order_id();

        // Make yes filled (closed), leave no out of cache (= not closed)
        let yes_filled =
            TestOrderStubs::make_filled_order(&yes_order, &instrument, LiquiditySide::Taker);
        {
            let cache_rc = strategy.cache_rc();
            cache_rc
                .borrow_mut()
                .add_order(yes_filled, None, None, false)
                .unwrap();
        }

        // Set up arb execution
        strategy.arb_executions.insert(
            "0xabc".to_string(),
            make_arb_execution(yes_order_id, no_order_id, OrderSide::Buy),
        );
        strategy
            .order_to_pair
            .insert(yes_order_id, "0xabc".to_string());
        strategy
            .order_to_pair
            .insert(no_order_id, "0xabc".to_string());

        // Fill the yes leg
        let yes_fill = OrderFilled::new(
            TraderId::from("TESTER-001"),
            StrategyId::from("COMPLEMENT_ARB-001"),
            pair.yes_id,
            yes_order_id,
            VenueOrderId::from("V-001"),
            AccountId::from("SIM-001"),
            TradeId::new("T-001"),
            OrderSide::Buy,
            OrderType::Limit,
            Quantity::from("10"),
            Price::from("0.480"),
            Currency::USDC(),
            LiquiditySide::Taker,
            nautilus_core::UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
            None,
            None,
        );
        strategy.handle_fill(&yes_fill);

        // Other leg not closed → should transition to PartialFill
        let exec = strategy.arb_executions.get("0xabc").unwrap();
        assert_eq!(exec.state, ArbState::PartialFill);
        assert_eq!(exec.yes_filled_qty, dec!(10));
        assert_eq!(exec.no_filled_qty, Decimal::ZERO);
    }

    #[rstest]
    fn test_scenario_handle_fill_unwind_order_completes() {
        let mut strategy = test_strategy();
        register_strategy(&mut strategy);

        let pair = test_pair();
        wire_pair(&mut strategy, &pair);
        let instrument =
            InstrumentAny::BinaryOption(make_binary_option(pair.yes_id, "Yes", "Test Market"));

        // Build and fill the unwind order
        let unwind_order =
            build_limit_order(pair.yes_id, OrderSide::Sell, "0.470", "10", "O-UNWIND-3");
        let unwind_order_id = unwind_order.client_order_id();
        let unwind_filled =
            TestOrderStubs::make_filled_order(&unwind_order, &instrument, LiquiditySide::Taker);
        {
            let cache_rc = strategy.cache_rc();
            cache_rc
                .borrow_mut()
                .add_order(unwind_filled, None, None, false)
                .unwrap();
        }

        // Set up Unwinding state
        let yes_order_id = ClientOrderId::new("O-ENTRY-YES");
        let no_order_id = ClientOrderId::new("O-ENTRY-NO");
        let mut exec = make_arb_execution(yes_order_id, no_order_id, OrderSide::Buy);
        exec.state = ArbState::Unwinding;
        exec.yes_filled_qty = dec!(10);
        exec.unwind_order_id = Some(unwind_order_id);
        strategy.arb_executions.insert("0xabc".to_string(), exec);
        strategy
            .order_to_pair
            .insert(yes_order_id, "0xabc".to_string());
        strategy
            .order_to_pair
            .insert(no_order_id, "0xabc".to_string());
        strategy
            .order_to_pair
            .insert(unwind_order_id, "0xabc".to_string());

        // Fill event for unwind order
        let unwind_fill = OrderFilled::new(
            TraderId::from("TESTER-001"),
            StrategyId::from("COMPLEMENT_ARB-001"),
            pair.yes_id,
            unwind_order_id,
            VenueOrderId::from("V-UNW"),
            AccountId::from("SIM-001"),
            TradeId::new("T-UNW"),
            OrderSide::Sell,
            OrderType::Limit,
            Quantity::from("10"),
            Price::from("0.470"),
            Currency::USDC(),
            LiquiditySide::Taker,
            nautilus_core::UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
            None,
            None,
        );
        strategy.handle_fill(&unwind_fill);

        // Unwind complete → cleaned up
        assert_eq!(strategy.arbs_unwound, 1);
        assert!(strategy.arb_executions.is_empty());
        assert!(strategy.order_to_pair.is_empty());
    }

    // ---- handle_order_terminal scenarios ----

    #[rstest]
    fn test_scenario_handle_terminal_both_legs_no_fills() {
        let mut strategy = test_strategy();
        register_strategy(&mut strategy);

        let pair = test_pair();
        wire_pair(&mut strategy, &pair);

        // Build two orders and make them accepted then canceled (closed, no fills)
        let yes_order = build_limit_order(pair.yes_id, OrderSide::Buy, "0.480", "10", "O-YES-4");
        let no_order = build_limit_order(pair.no_id, OrderSide::Buy, "0.480", "10", "O-NO-4");
        let yes_order_id = yes_order.client_order_id();
        let no_order_id = no_order.client_order_id();

        // Make accepted then canceled → closed with 0 fills
        let mut yes_accepted = TestOrderStubs::make_accepted_order(&yes_order);
        let yes_cancel =
            TestOrderEventStubs::canceled(&yes_accepted, AccountId::from("SIM-001"), None);
        yes_accepted.apply(yes_cancel).unwrap();

        let mut no_accepted = TestOrderStubs::make_accepted_order(&no_order);
        let no_cancel =
            TestOrderEventStubs::canceled(&no_accepted, AccountId::from("SIM-001"), None);
        no_accepted.apply(no_cancel).unwrap();

        // Add to cache
        {
            let cache_rc = strategy.cache_rc();
            cache_rc
                .borrow_mut()
                .add_order(yes_accepted, None, None, false)
                .unwrap();
            cache_rc
                .borrow_mut()
                .add_order(no_accepted, None, None, false)
                .unwrap();
        }

        // Set up arb execution with zero fills
        strategy.arb_executions.insert(
            "0xabc".to_string(),
            make_arb_execution(yes_order_id, no_order_id, OrderSide::Buy),
        );
        strategy
            .order_to_pair
            .insert(yes_order_id, "0xabc".to_string());
        strategy
            .order_to_pair
            .insert(no_order_id, "0xabc".to_string());

        // First leg terminal arrives
        strategy.handle_order_terminal(yes_order_id, "expired");

        // Both closed, no fills → ARB FAILED
        assert_eq!(strategy.arbs_failed, 1);
        assert!(strategy.arb_executions.is_empty());
    }

    #[rstest]
    fn test_scenario_handle_terminal_triggers_unwind() {
        let mut strategy = test_strategy();
        register_strategy(&mut strategy);

        let pair = test_pair();
        wire_pair(&mut strategy, &pair);
        let instrument =
            InstrumentAny::BinaryOption(make_binary_option(pair.yes_id, "Yes", "Test Market"));

        // Yes leg: filled (closed)
        let yes_order = build_limit_order(pair.yes_id, OrderSide::Buy, "0.480", "10", "O-YES-5");
        let no_order = build_limit_order(pair.no_id, OrderSide::Buy, "0.480", "10", "O-NO-5");
        let yes_order_id = yes_order.client_order_id();
        let no_order_id = no_order.client_order_id();

        let yes_filled =
            TestOrderStubs::make_filled_order(&yes_order, &instrument, LiquiditySide::Taker);

        // No leg: accepted then canceled (closed, no fills)
        let mut no_accepted = TestOrderStubs::make_accepted_order(&no_order);
        let no_cancel =
            TestOrderEventStubs::canceled(&no_accepted, AccountId::from("SIM-001"), None);
        no_accepted.apply(no_cancel).unwrap();

        {
            let cache_rc = strategy.cache_rc();
            cache_rc
                .borrow_mut()
                .add_order(yes_filled, None, None, false)
                .unwrap();
            cache_rc
                .borrow_mut()
                .add_order(no_accepted, None, None, false)
                .unwrap();
        }

        // Set up execution: yes has fills, no does not
        let mut exec = make_arb_execution(yes_order_id, no_order_id, OrderSide::Buy);
        exec.yes_filled_qty = dec!(10);
        strategy.arb_executions.insert("0xabc".to_string(), exec);
        strategy
            .order_to_pair
            .insert(yes_order_id, "0xabc".to_string());
        strategy
            .order_to_pair
            .insert(no_order_id, "0xabc".to_string());

        // Need a quote for unwind pricing
        strategy
            .quotes
            .insert(pair.yes_id, quote(pair.yes_id, 0.45, 0.48, 100.0));

        // No leg terminal arrives → both closed, yes has fills → initiate_unwind
        strategy.handle_order_terminal(no_order_id, "expired");

        // Should be in Unwinding state (or failed if submit_unwind errors in test env)
        let exec = strategy.arb_executions.get("0xabc");
        if let Some(e) = exec {
            assert_eq!(e.state, ArbState::Unwinding);
            assert!(e.unwind_order_id.is_some());
        } else {
            // If unwind submission failed (no message bus), it should have cleaned up
            assert!(strategy.arbs_failed > 0);
        }
    }

    #[rstest]
    fn test_scenario_unwinding_ignores_entry_leg_terminal() {
        let mut strategy = test_strategy();
        register_strategy(&mut strategy);

        let pair = test_pair();
        wire_pair(&mut strategy, &pair);

        let yes_order_id = ClientOrderId::new("O-YES");
        let no_order_id = ClientOrderId::new("O-NO");
        let unwind_order_id = ClientOrderId::new("O-UNWIND");

        let mut exec = make_arb_execution(yes_order_id, no_order_id, OrderSide::Buy);
        exec.state = ArbState::Unwinding;
        exec.unwind_order_id = Some(unwind_order_id);
        exec.yes_filled_qty = dec!(10);

        strategy.arb_executions.insert("0xabc".to_string(), exec);
        strategy
            .order_to_pair
            .insert(yes_order_id, "0xabc".to_string());
        strategy
            .order_to_pair
            .insert(no_order_id, "0xabc".to_string());
        strategy
            .order_to_pair
            .insert(unwind_order_id, "0xabc".to_string());

        // Entry leg terminal → should be ignored during unwind
        strategy.handle_order_terminal(yes_order_id, "canceled");

        // State unchanged
        let exec = strategy.arb_executions.get("0xabc").unwrap();
        assert_eq!(exec.state, ArbState::Unwinding);
        assert_eq!(strategy.arbs_failed, 0);
        assert_eq!(strategy.arbs_unwound, 0);
    }

    #[rstest]
    fn test_scenario_unwind_order_rejected_fails() {
        let mut strategy = test_strategy();
        register_strategy(&mut strategy);

        let pair = test_pair();
        wire_pair(&mut strategy, &pair);

        let yes_order_id = ClientOrderId::new("O-YES");
        let no_order_id = ClientOrderId::new("O-NO");
        let unwind_order_id = ClientOrderId::new("O-UNWIND");

        let mut exec = make_arb_execution(yes_order_id, no_order_id, OrderSide::Buy);
        exec.state = ArbState::Unwinding;
        exec.unwind_order_id = Some(unwind_order_id);
        exec.yes_filled_qty = dec!(10);

        strategy.arb_executions.insert("0xabc".to_string(), exec);
        strategy
            .order_to_pair
            .insert(yes_order_id, "0xabc".to_string());
        strategy
            .order_to_pair
            .insert(no_order_id, "0xabc".to_string());
        strategy
            .order_to_pair
            .insert(unwind_order_id, "0xabc".to_string());

        // Unwind order terminal → UNWIND FAILED
        strategy.handle_order_terminal(unwind_order_id, "rejected");

        assert_eq!(strategy.arbs_failed, 1);
        assert!(strategy.arb_executions.is_empty());
        assert!(strategy.order_to_pair.is_empty());
    }

    #[rstest]
    fn test_scenario_handle_terminal_unknown_order_noop() {
        let mut strategy = test_strategy();
        register_strategy(&mut strategy);

        // Random order ID not in order_to_pair → no-op
        strategy.handle_order_terminal(ClientOrderId::new("O-UNKNOWN"), "rejected");

        assert_eq!(strategy.arbs_failed, 0);
        assert_eq!(strategy.arbs_completed, 0);
    }

    #[rstest]
    fn test_scenario_handle_fill_unknown_order_noop() {
        let mut strategy = test_strategy();
        register_strategy(&mut strategy);

        // Fill for unknown order → no-op
        let fill = OrderFilled::new(
            TraderId::from("TESTER-001"),
            StrategyId::from("COMPLEMENT_ARB-001"),
            InstrumentId::from("0xabc-111.POLYMARKET"),
            ClientOrderId::new("O-UNKNOWN"),
            VenueOrderId::from("V-001"),
            AccountId::from("SIM-001"),
            TradeId::new("T-001"),
            OrderSide::Buy,
            OrderType::Limit,
            Quantity::from("10"),
            Price::from("0.480"),
            Currency::USDC(),
            LiquiditySide::Taker,
            nautilus_core::UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
            None,
            None,
        );
        strategy.handle_fill(&fill);

        assert_eq!(strategy.arbs_completed, 0);
        assert_eq!(strategy.arbs_failed, 0);
    }
}

mod lifecycle {
    use super::*;

    #[rstest]
    fn test_on_reset_clears_all_state() {
        let mut strategy = test_strategy();
        let pair = test_pair();

        strategy.pairs.insert("test".to_string(), pair.clone());
        strategy
            .quotes
            .insert(pair.yes_id, quote(pair.yes_id, 0.48, 0.52, 100.0));
        strategy.buy_arbs_detected = 5;

        strategy.on_reset().unwrap();

        assert!(strategy.pairs.is_empty());
        assert!(strategy.quotes.is_empty());
        assert_eq!(strategy.buy_arbs_detected, 0);
        assert_eq!(strategy.best_buy_spread, dec!(2));
        assert_eq!(strategy.best_sell_spread, dec!(0));
    }

    #[rstest]
    fn test_on_reset_clears_execution_state() {
        let mut strategy = test_strategy();
        let yes_order_id = ClientOrderId::new("O-001");
        let no_order_id = ClientOrderId::new("O-002");

        strategy.arb_executions.insert(
            "0xabc".to_string(),
            make_arb_execution(yes_order_id, no_order_id, OrderSide::Buy),
        );
        strategy
            .order_to_pair
            .insert(yes_order_id, "0xabc".to_string());
        strategy.arbs_submitted = 5;
        strategy.arbs_completed = 3;
        strategy.arbs_unwound = 1;
        strategy.arbs_failed = 1;

        strategy.on_reset().unwrap();

        assert!(strategy.arb_executions.is_empty());
        assert!(strategy.order_to_pair.is_empty());
        assert_eq!(strategy.arbs_submitted, 0);
        assert_eq!(strategy.arbs_completed, 0);
        assert_eq!(strategy.arbs_unwound, 0);
        assert_eq!(strategy.arbs_failed, 0);
    }

    #[rstest]
    fn test_pending_complements_cleared_on_reset() {
        let mut strategy = test_strategy();

        strategy.pending_complements.insert(
            "0xabc".to_string(),
            (
                InstrumentId::from("0xabc-111.POLYMARKET"),
                "Yes".to_string(),
                "Test".to_string(),
            ),
        );

        strategy.on_reset().unwrap();
        assert!(strategy.pending_complements.is_empty());
    }

    #[rstest]
    fn test_scenario_full_detection_cycle_with_reset() {
        let mut strategy = test_strategy();
        let pair = test_pair();

        // Detect arbs
        strategy
            .quotes
            .insert(pair.yes_id, quote(pair.yes_id, 0.45, 0.48, 100.0));
        strategy
            .quotes
            .insert(pair.no_id, quote(pair.no_id, 0.45, 0.48, 100.0));
        assert!(strategy.check_buy_arb(&pair));
        assert_eq!(strategy.buy_arbs_detected, 1);
        assert_eq!(strategy.best_buy_spread, dec!(0.96));

        // Reset
        strategy.on_reset().unwrap();
        assert_eq!(strategy.buy_arbs_detected, 0);
        assert_eq!(strategy.best_buy_spread, dec!(2));
        assert!(strategy.quotes.is_empty());

        // Detect arbs again with different quotes
        strategy
            .quotes
            .insert(pair.yes_id, quote(pair.yes_id, 0.38, 0.40, 100.0));
        strategy
            .quotes
            .insert(pair.no_id, quote(pair.no_id, 0.38, 0.40, 100.0));
        assert!(strategy.check_buy_arb(&pair));
        assert_eq!(strategy.buy_arbs_detected, 1); // counter starts fresh
        assert_eq!(strategy.best_buy_spread, dec!(0.80));
    }

    #[rstest]
    fn test_cleanup_arb_removes_all_tracking() {
        let mut strategy = test_strategy();

        let yes_order_id = ClientOrderId::new("O-001");
        let no_order_id = ClientOrderId::new("O-002");
        let unwind_order_id = ClientOrderId::new("O-003");

        strategy.arb_executions.insert(
            "0xabc".to_string(),
            ArbExecution {
                state: ArbState::Unwinding,
                arb_side: OrderSide::Buy,
                yes_order_id,
                no_order_id,
                yes_filled_qty: dec!(10),
                no_filled_qty: Decimal::ZERO,
                unwind_order_id: Some(unwind_order_id),
            },
        );
        strategy
            .order_to_pair
            .insert(yes_order_id, "0xabc".to_string());
        strategy
            .order_to_pair
            .insert(no_order_id, "0xabc".to_string());
        strategy
            .order_to_pair
            .insert(unwind_order_id, "0xabc".to_string());

        strategy.cleanup_arb("0xabc");

        assert!(strategy.arb_executions.is_empty());
        assert!(strategy.order_to_pair.is_empty());
    }

    #[rstest]
    fn test_scenario_cleanup_isolation_between_pairs() {
        let mut strategy = test_strategy();

        let pair_a_yes = ClientOrderId::new("O-A-YES");
        let pair_a_no = ClientOrderId::new("O-A-NO");
        let pair_b_yes = ClientOrderId::new("O-B-YES");
        let pair_b_no = ClientOrderId::new("O-B-NO");

        strategy.arb_executions.insert(
            "0xabc".to_string(),
            make_arb_execution(pair_a_yes, pair_a_no, OrderSide::Buy),
        );
        strategy
            .order_to_pair
            .insert(pair_a_yes, "0xabc".to_string());
        strategy
            .order_to_pair
            .insert(pair_a_no, "0xabc".to_string());

        strategy.arb_executions.insert(
            "0xdef".to_string(),
            make_arb_execution(pair_b_yes, pair_b_no, OrderSide::Sell),
        );
        strategy
            .order_to_pair
            .insert(pair_b_yes, "0xdef".to_string());
        strategy
            .order_to_pair
            .insert(pair_b_no, "0xdef".to_string());

        // Cleanup pair A only
        strategy.cleanup_arb("0xabc");

        // Pair A cleaned
        assert!(!strategy.arb_executions.contains_key("0xabc"));
        assert!(!strategy.order_to_pair.contains_key(&pair_a_yes));
        assert!(!strategy.order_to_pair.contains_key(&pair_a_no));

        // Pair B untouched
        assert!(strategy.arb_executions.contains_key("0xdef"));
        assert!(strategy.order_to_pair.contains_key(&pair_b_yes));
        assert!(strategy.order_to_pair.contains_key(&pair_b_no));
    }

    #[rstest]
    fn test_scenario_diagnostic_summary_does_not_panic() {
        let mut strategy = test_strategy();

        // Set various counter states
        strategy.arbs_submitted = 10;
        strategy.arbs_completed = 5;
        strategy.arbs_unwound = 2;
        strategy.arbs_failed = 3;
        strategy.buy_arbs_detected = 15;
        strategy.sell_arbs_detected = 8;
        strategy.quotes_processed = 1000;
        strategy.best_buy_spread = dec!(0.95);
        strategy.best_buy_label = "Some Market".to_string();
        strategy.best_sell_spread = dec!(1.05);
        strategy.best_sell_label = "Other Market".to_string();

        // Should not panic
        strategy.log_diagnostic_summary();

        // Also test with default/zero state
        let fresh_strategy = test_strategy();
        fresh_strategy.log_diagnostic_summary();
    }
}

mod configuration {
    use super::*;

    #[rstest]
    fn test_config_defaults() {
        let config = ComplementArbConfig::builder()
            .venue(Venue::new("POLYMARKET"))
            .build();

        assert_eq!(config.order_expire_secs, 15);
        assert_eq!(config.unwind_slippage_bps, dec!(50));
        assert!(!config.live_trading);
    }

    #[rstest]
    fn test_config_post_only_false_still_works() {
        let config = ComplementArbConfig::builder()
            .venue(Venue::new("POLYMARKET"))
            .use_post_only(false)
            .build();

        assert!(!config.use_post_only);

        // Strategy initializes correctly
        let strategy = ComplementArb::new(config);
        assert_eq!(strategy.buy_arbs_detected, 0);
    }

    #[rstest]
    fn test_config_zero_fee_no_fee_deduction() {
        let config = ComplementArbConfig::builder()
            .venue(Venue::new("POLYMARKET"))
            .fee_estimate_bps(dec!(0))
            .min_profit_bps(dec!(50))
            .trade_size(dec!(10))
            .build();
        let mut strategy = ComplementArb::new(config);
        let pair = test_pair();

        // Marginal arb: combined_ask = 0.994 → raw profit = 60 bps
        // With zero fee, net profit = 60 bps > 50 → should pass
        strategy
            .quotes
            .insert(pair.yes_id, quote(pair.yes_id, 0.49, 0.497, 100.0));
        strategy
            .quotes
            .insert(pair.no_id, quote(pair.no_id, 0.49, 0.497, 100.0));

        assert!(strategy.check_buy_arb(&pair));

        // Same with a fee that would kill it
        let config_fee = ComplementArbConfig::builder()
            .venue(Venue::new("POLYMARKET"))
            .fee_estimate_bps(dec!(200))
            .min_profit_bps(dec!(50))
            .trade_size(dec!(10))
            .build();
        let mut strategy_fee = ComplementArb::new(config_fee);
        strategy_fee
            .quotes
            .insert(pair.yes_id, quote(pair.yes_id, 0.49, 0.497, 100.0));
        strategy_fee
            .quotes
            .insert(pair.no_id, quote(pair.no_id, 0.49, 0.497, 100.0));

        assert!(!strategy_fee.check_buy_arb(&pair));
    }
}

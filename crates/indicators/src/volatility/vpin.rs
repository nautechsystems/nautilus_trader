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

//! Volume-synchronized probability of informed trading (VPIN).

use std::{
    collections::VecDeque,
    fmt::{Debug, Display},
};

use nautilus_core::correctness::FAILED;
use nautilus_model::{
    data::{Bar, QuoteTick, TradeTick},
    enums::AggressorSide,
};

use crate::{
    indicator::Indicator,
    support::{MAX_PERIOD, ShiftedMoments},
};

/// The default number of bar close changes used to estimate sigma for bulk volume classification.
pub const VPIN_DEFAULT_SIGMA_WINDOW: usize = 50;

/// Volume-synchronized probability of informed trading (VPIN).
///
/// Easley, Lopez de Prado and O'Hara (2012), "Flow Toxicity and Liquidity in a High-frequency
/// World", Review of Financial Studies 25(5). Classified volume fills consecutive buckets of
/// exactly `bucket_volume` units. A print that overflows a bucket is split, keeping its buy share,
/// and the remainder carries into the next bucket. The value is the mean order imbalance
/// `|V_buy - V_sell| / bucket_volume` over the last `window` full buckets, so it lies in `[0, 1]`.
///
/// Volume is classified by input type:
/// - Trades use the venue aggressor side. A trade with no aggressor falls back to the tick rule
///   (an uptick is a buy, a downtick a sell, an unchanged price repeats the last sign); a trade the
///   tick rule cannot sign yet stays out of the buckets.
/// - Bars use bulk volume classification. The buy share of the bar volume is `Phi(dP / sigma)`,
///   where `dP` is the change from the previous close and `sigma` the sample standard deviation of
///   the last `sigma_window` close changes, the current one included. Bars that arrive before
///   `sigma_window` changes exist stay out of the buckets.
/// - [`Vpin::update_raw`] takes buy and sell volume classified by the caller.
///
/// `count` is the number of completed buckets. ELO (2012) size buckets at one fiftieth of average
/// daily volume and average over 50 buckets.
#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.indicators")
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.indicators")
)]
pub struct Vpin {
    pub bucket_volume: f64,
    pub window: usize,
    pub sigma_window: usize,
    pub value: f64,
    pub count: usize,
    pub initialized: bool,
    has_inputs: bool,
    bucket_buy: f64,
    bucket_sell: f64,
    bucket_fill: f64,
    imbalances: VecDeque<f64>,
    imbalance_sum: f64,
    last_trade_price: Option<f64>,
    last_tick_is_buy: Option<bool>,
    last_close: Option<f64>,
    close_changes: VecDeque<f64>,
    moments: ShiftedMoments,
}

impl Display for Vpin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}({},{},{})",
            self.name(),
            self.bucket_volume,
            self.window,
            self.sigma_window,
        )
    }
}

impl Indicator for Vpin {
    fn name(&self) -> String {
        stringify!(Vpin).to_string()
    }

    fn has_inputs(&self) -> bool {
        self.has_inputs
    }

    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_quote(&mut self, _quote: &QuoteTick) -> anyhow::Result<()> {
        Ok(())
    }

    fn handle_trade(&mut self, trade: &TradeTick) {
        self.update_trade(
            trade.price.as_f64(),
            trade.size.as_f64(),
            trade.aggressor_side,
        );
    }

    fn handle_bar(&mut self, bar: &Bar) {
        self.update_bar(bar.close.as_f64(), bar.volume.as_f64());
    }

    fn reset(&mut self) {
        self.value = 0.0;
        self.count = 0;
        self.initialized = false;
        self.has_inputs = false;
        self.bucket_buy = 0.0;
        self.bucket_sell = 0.0;
        self.bucket_fill = 0.0;
        self.imbalances.clear();
        self.imbalance_sum = 0.0;
        self.last_trade_price = None;
        self.last_tick_is_buy = None;
        self.last_close = None;
        self.close_changes.clear();
        self.moments.reset();
    }
}

impl Vpin {
    /// Creates a new [`Vpin`] instance.
    ///
    /// # Panics
    ///
    /// Panics if `bucket_volume` is not finite and positive, if `window` is zero or above the
    /// maximum period, or if `sigma_window` is below 2 or above the maximum period.
    #[must_use]
    pub fn new(bucket_volume: f64, window: usize, sigma_window: Option<usize>) -> Self {
        Self::new_checked(bucket_volume, window, sigma_window).expect(FAILED)
    }

    pub(crate) fn new_checked(
        bucket_volume: f64,
        window: usize,
        sigma_window: Option<usize>,
    ) -> anyhow::Result<Self> {
        let sigma_window = sigma_window.unwrap_or(VPIN_DEFAULT_SIGMA_WINDOW);
        anyhow::ensure!(
            bucket_volume.is_finite() && bucket_volume > 0.0,
            "Vpin: bucket_volume must be finite and > 0 (received {bucket_volume})"
        );
        anyhow::ensure!(window <= MAX_PERIOD, "window cannot exceed {MAX_PERIOD}");
        anyhow::ensure!(window > 0, "Vpin: window must be > 0 (received {window})");
        anyhow::ensure!(
            sigma_window <= MAX_PERIOD,
            "sigma_window cannot exceed {MAX_PERIOD}"
        );
        anyhow::ensure!(
            sigma_window >= 2,
            "Vpin: sigma_window must be >= 2 (received {sigma_window})"
        );
        Ok(Self {
            bucket_volume,
            window,
            sigma_window,
            value: 0.0,
            count: 0,
            initialized: false,
            has_inputs: false,
            bucket_buy: 0.0,
            bucket_sell: 0.0,
            bucket_fill: 0.0,
            imbalances: VecDeque::with_capacity(window.min(1024)),
            imbalance_sum: 0.0,
            last_trade_price: None,
            last_tick_is_buy: None,
            last_close: None,
            close_changes: VecDeque::with_capacity(sigma_window.min(1024)),
            moments: ShiftedMoments::new(),
        })
    }

    /// Updates the indicator with a trade price, size and aggressor side.
    ///
    /// Trades with [`AggressorSide::NoAggressor`] are signed by the tick rule. Every trade with a
    /// finite price and positive size advances the tick rule, so its history stays complete
    /// whichever side source signs a given trade.
    pub fn update_trade(&mut self, price: f64, size: f64, aggressor_side: AggressorSide) {
        if !price.is_finite() || !size.is_finite() || size <= 0.0 {
            return;
        }
        self.has_inputs = true;

        let by_tick = match self.last_trade_price {
            Some(previous) if price > previous => Some(true),
            Some(previous) if price < previous => Some(false),
            Some(_) => self.last_tick_is_buy,
            None => None,
        };
        self.last_trade_price = Some(price);

        if by_tick.is_some() {
            self.last_tick_is_buy = by_tick;
        }

        let is_buy = match aggressor_side {
            AggressorSide::Buy => Some(true),
            AggressorSide::Sell => Some(false),
            AggressorSide::NoAggressor => by_tick,
        };

        match is_buy {
            Some(true) => self.update_raw(size, 0.0),
            Some(false) => self.update_raw(0.0, size),
            None => {}
        }
    }

    /// Updates the indicator with a bar close and volume using bulk volume classification.
    pub fn update_bar(&mut self, close: f64, volume: f64) {
        if !close.is_finite() || !volume.is_finite() || volume < 0.0 {
            return;
        }
        self.has_inputs = true;

        let Some(previous) = self.last_close.replace(close) else {
            return;
        };
        let change = close - previous;

        if self.close_changes.len() == self.sigma_window
            && let Some(old) = self.close_changes.pop_front()
        {
            self.moments.evict(old);
        }
        self.close_changes.push_back(change);
        self.moments.push(change);
        if self.moments.needs_reseed(self.sigma_window) {
            self.moments.reseed(&self.close_changes);
        }

        if self.close_changes.len() < self.sigma_window {
            return;
        }
        // `std_dev` is the population value, scale it to the sample (n - 1) standard deviation
        let n = self.sigma_window as f64;
        let sigma = self.moments.std_dev(self.sigma_window) * (n / (n - 1.0)).sqrt();
        // A window with no dispersion carries no direction: split the bar evenly
        let buy_share = if sigma > 0.0 {
            normal_cdf(change / sigma)
        } else {
            0.5
        };
        self.update_raw(volume * buy_share, volume * (1.0 - buy_share));
    }

    /// Updates the indicator with buy and sell volume classified by the caller.
    ///
    /// Both sides may be non-zero, for example a bulk-classified bar. When the input spans
    /// several buckets, each portion keeps the input's buy share.
    pub fn update_raw(&mut self, buy_volume: f64, sell_volume: f64) {
        if !buy_volume.is_finite()
            || !sell_volume.is_finite()
            || buy_volume < 0.0
            || sell_volume < 0.0
        {
            return;
        }
        let total = buy_volume + sell_volume;
        if total <= 0.0 {
            return;
        }
        self.has_inputs = true;

        let buy_share = buy_volume / total;
        // Absorbs rounding so a print that fills a bucket exactly does not leave a sliver behind
        let epsilon = self.bucket_volume * 1e-12;
        let mut remaining = total;
        while remaining > epsilon {
            let space = self.bucket_volume - self.bucket_fill;
            if remaining < space - epsilon {
                self.bucket_buy += remaining * buy_share;
                self.bucket_sell += remaining * (1.0 - buy_share);
                self.bucket_fill += remaining;
                break;
            }
            self.bucket_buy += space * buy_share;
            self.bucket_sell += space * (1.0 - buy_share);
            remaining -= space;
            self.close_bucket();
        }
    }

    fn close_bucket(&mut self) {
        let imbalance = ((self.bucket_buy - self.bucket_sell).abs() / self.bucket_volume).min(1.0);
        self.bucket_buy = 0.0;
        self.bucket_sell = 0.0;
        self.bucket_fill = 0.0;
        self.count += 1;

        if self.imbalances.len() == self.window
            && let Some(old) = self.imbalances.pop_front()
        {
            self.imbalance_sum -= old;
        }
        self.imbalances.push_back(imbalance);
        self.imbalance_sum += imbalance;
        // Re-sum once per window so rounding in the running sum cannot accumulate
        if self.count.is_multiple_of(self.window) {
            self.imbalance_sum = self.imbalances.iter().sum();
        }

        if self.imbalances.len() == self.window {
            self.value = (self.imbalance_sum / self.window as f64).clamp(0.0, 1.0);
            self.initialized = true;
        }
    }
}

/// Standard normal CDF, absolute error below 1e-14.
///
/// Hart's (1968) double-precision rational approximation, in the form given by G. West,
/// "Better approximations to cumulative normal functions" (Wilmott, 2005).
fn normal_cdf(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    let z = x.abs();
    let tail = if z > 37.0 {
        0.0
    } else {
        let e = (-0.5 * z * z).exp();
        if z < 7.071_067_811_865_47 {
            let mut num = 3.526_249_659_989_11e-2_f64.mul_add(z, 0.700_383_064_443_688);
            num = num.mul_add(z, 6.373_962_203_531_65);
            num = num.mul_add(z, 33.912_866_078_383);
            num = num.mul_add(z, 112.079_291_497_871);
            num = num.mul_add(z, 221.213_596_169_931);
            num = num.mul_add(z, 220.206_867_912_376);
            let mut den = 8.838_834_764_831_84e-2_f64.mul_add(z, 1.755_667_163_182_64);
            den = den.mul_add(z, 16.064_177_579_207);
            den = den.mul_add(z, 86.780_732_202_946_1);
            den = den.mul_add(z, 296.564_248_779_674);
            den = den.mul_add(z, 637.333_633_378_831);
            den = den.mul_add(z, 793.826_512_519_948);
            den = den.mul_add(z, 440.413_735_824_752);
            e * num / den
        } else {
            let mut b = z + 0.65;
            b = z + 4.0 / b;
            b = z + 3.0 / b;
            b = z + 2.0 / b;
            b = z + 1.0 / b;
            e / b / 2.506_628_274_631
        }
    };

    if x > 0.0 { 1.0 - tail } else { tail }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use rstest::rstest;

    use super::*;
    use crate::stubs::{bar_ethusdt_binance_minute_bid, stub_quote, stub_trade, vpin_10_2};

    const TOLERANCE: f64 = 1e-12;

    fn feed_sides(indicator: &mut Vpin, trades: &[(AggressorSide, f64)]) {
        for (index, &(side, size)) in trades.iter().enumerate() {
            indicator.update_trade(100.0 + index as f64, size, side);
        }
    }

    #[rstest]
    fn test_name_and_display(vpin_10_2: Vpin) {
        assert_eq!(vpin_10_2.name(), "Vpin");
        assert_eq!(format!("{vpin_10_2}"), "Vpin(10,2,50)");
        assert_eq!(vpin_10_2.bucket_volume, 10.0);
        assert_eq!(vpin_10_2.window, 2);
        assert_eq!(vpin_10_2.sigma_window, VPIN_DEFAULT_SIGMA_WINDOW);
        assert_eq!(vpin_10_2.value, 0.0);
        assert_eq!(vpin_10_2.count, 0);
        assert!(!vpin_10_2.initialized());
        assert!(!vpin_10_2.has_inputs());
    }

    #[rstest]
    #[case(0.0, 2, None)]
    #[case(-1.0, 2, None)]
    #[case(f64::NAN, 2, None)]
    #[case(f64::INFINITY, 2, None)]
    #[case(10.0, 0, None)]
    #[case(10.0, MAX_PERIOD + 1, None)]
    #[case(10.0, 2, Some(1))]
    #[case(10.0, 2, Some(MAX_PERIOD + 1))]
    fn test_invalid_parameters_are_rejected(
        #[case] bucket_volume: f64,
        #[case] window: usize,
        #[case] sigma_window: Option<usize>,
    ) {
        assert!(Vpin::new_checked(bucket_volume, window, sigma_window).is_err());
    }

    #[rstest]
    #[should_panic(expected = "window must be > 0")]
    fn test_zero_window_panics() {
        let _ = Vpin::new(10.0, 0, None);
    }

    #[rstest]
    fn test_aggressor_sides_fill_buckets() {
        // Bucket 1: buy 6 + sell 4 -> |6 - 4| / 10 = 0.2. Bucket 2: sell 10 -> 1.0.
        // VPIN over two buckets = (0.2 + 1.0) / 2 = 0.6.
        let mut indicator = Vpin::new(10.0, 2, None);
        feed_sides(
            &mut indicator,
            &[(AggressorSide::Buy, 6.0), (AggressorSide::Sell, 4.0)],
        );

        assert_eq!(indicator.count, 1);
        assert!(!indicator.initialized());
        assert_eq!(indicator.value, 0.0);

        feed_sides(&mut indicator, &[(AggressorSide::Sell, 10.0)]);

        assert_eq!(indicator.count, 2);
        assert!(indicator.initialized());
        assert!((indicator.value - 0.6).abs() < TOLERANCE);
    }

    #[rstest]
    fn test_alternating_unit_trades_in_odd_buckets() {
        // Ten buys and ten sells of one unit, alternating, into buckets of five units.
        // Every bucket holds three of one side and two of the other: |3 - 2| / 5 = 0.2.
        let mut indicator = Vpin::new(5.0, 4, None);

        for index in 0..20 {
            let side = if index % 2 == 0 {
                AggressorSide::Buy
            } else {
                AggressorSide::Sell
            };
            indicator.update_trade(100.0, 1.0, side);
        }

        assert_eq!(indicator.count, 4);
        assert!((indicator.value - 0.2).abs() < TOLERANCE);
    }

    #[rstest]
    fn test_alternating_unit_trades_in_even_buckets() {
        // Buckets of four units each hold two buys and two sells: VPIN is exactly zero
        let mut indicator = Vpin::new(4.0, 5, None);

        for index in 0..20 {
            let side = if index % 2 == 0 {
                AggressorSide::Buy
            } else {
                AggressorSide::Sell
            };
            indicator.update_trade(100.0, 1.0, side);
        }

        assert_eq!(indicator.count, 5);
        assert!(indicator.initialized());
        assert_eq!(indicator.value, 0.0);
    }

    #[rstest]
    fn test_large_trade_spans_buckets() {
        // Buy 25 closes two all-buy buckets (1.0, 1.0) and leaves 5 bought in the third.
        // Sell 15 completes the third bucket at 5 vs 5 (0.0) and fills a fourth with sells (1.0).
        // VPIN over four buckets = (1 + 1 + 0 + 1) / 4 = 0.75.
        let mut indicator = Vpin::new(10.0, 4, None);
        feed_sides(
            &mut indicator,
            &[(AggressorSide::Buy, 25.0), (AggressorSide::Sell, 15.0)],
        );

        assert_eq!(indicator.count, 4);
        assert!((indicator.value - 0.75).abs() < TOLERANCE);
    }

    #[rstest]
    fn test_no_aggressor_uses_tick_rule() {
        // 100 x 4: no previous price, cannot be signed, stays out of the buckets.
        // 101 x 6: uptick, buy 6. 100 x 8: downtick, sell 4 closes bucket 1 (6 vs 4 -> 0.2)
        // and 4 carry over. 100 x 6: zero tick repeats sell, bucket 2 is all sells (1.0).
        let mut indicator = Vpin::new(10.0, 2, None);
        indicator.update_trade(100.0, 4.0, AggressorSide::NoAggressor);

        assert!(indicator.has_inputs());
        assert_eq!(indicator.count, 0);

        indicator.update_trade(101.0, 6.0, AggressorSide::NoAggressor);
        indicator.update_trade(100.0, 8.0, AggressorSide::NoAggressor);
        indicator.update_trade(100.0, 6.0, AggressorSide::NoAggressor);

        assert_eq!(indicator.count, 2);
        assert!((indicator.value - 0.6).abs() < TOLERANCE);
    }

    #[rstest]
    fn test_tick_rule_history_includes_aggressor_trades() {
        // The aggressor trade at 101 still sets the tick history, so the later
        // unsigned trade at 100 is a downtick (sell)
        let mut indicator = Vpin::new(10.0, 1, None);
        indicator.update_trade(101.0, 5.0, AggressorSide::Buy);
        indicator.update_trade(100.0, 5.0, AggressorSide::NoAggressor);

        assert_eq!(indicator.count, 1);
        assert_eq!(indicator.value, 0.0);
    }

    #[rstest]
    fn test_bulk_volume_classification() {
        // Closes 100, 101, 100 give changes +1 and -1: sample sigma = sqrt(2).
        // The third bar's buy share is Phi(-1 / sqrt(2)) = erfc(1/2) / 2, so its imbalance is
        // |2 Phi(-1 / sqrt(2)) - 1| = erf(1/2) = 0.520499877813046537682746653892.
        let mut indicator = Vpin::new(10.0, 1, Some(2));
        indicator.update_bar(100.0, 10.0);
        indicator.update_bar(101.0, 10.0);

        assert!(indicator.has_inputs());
        assert_eq!(indicator.count, 0);

        indicator.update_bar(100.0, 10.0);

        assert_eq!(indicator.count, 1);
        assert!((indicator.value - 0.520_499_877_813_046_5).abs() < 1e-14);
    }

    #[rstest]
    fn test_bulk_volume_classification_flat_window_splits_evenly() {
        let mut indicator = Vpin::new(10.0, 1, Some(2));
        for _ in 0..4 {
            indicator.update_bar(100.0, 10.0);
        }

        assert_eq!(indicator.count, 2);
        assert_eq!(indicator.value, 0.0);
    }

    #[rstest]
    fn test_update_raw_split_keeps_buy_share() {
        // 15 buy + 5 sell is a 0.75 buy share: each 10-unit bucket gets 7.5 vs 2.5 -> 0.5
        let mut indicator = Vpin::new(10.0, 2, None);
        indicator.update_raw(15.0, 5.0);

        assert_eq!(indicator.count, 2);
        assert!((indicator.value - 0.5).abs() < TOLERANCE);
    }

    #[rstest]
    #[case(f64::NAN, 1.0)]
    #[case(1.0, f64::INFINITY)]
    #[case(-1.0, 2.0)]
    #[case(0.0, 0.0)]
    fn test_rejected_raw_input_leaves_state_unchanged(#[case] buy: f64, #[case] sell: f64) {
        let mut indicator = Vpin::new(10.0, 1, None);
        indicator.update_raw(buy, sell);

        assert!(!indicator.has_inputs());
        assert_eq!(indicator.count, 0);
    }

    #[rstest]
    #[case(f64::NAN, 1.0)]
    #[case(100.0, 0.0)]
    #[case(100.0, -1.0)]
    #[case(100.0, f64::NAN)]
    fn test_rejected_trade_leaves_tick_history_unchanged(#[case] price: f64, #[case] size: f64) {
        let mut indicator = Vpin::new(10.0, 1, None);
        indicator.update_trade(price, size, AggressorSide::NoAggressor);

        assert!(!indicator.has_inputs());

        // Had the rejected trade set a previous price, this trade would be signed
        indicator.update_trade(99.0, 10.0, AggressorSide::NoAggressor);
        assert_eq!(indicator.count, 0);
    }

    #[rstest]
    fn test_handle_trade_uses_aggressor_side(stub_trade: TradeTick) {
        let mut indicator = Vpin::new(1.0, 1, None);
        indicator.handle_trade(&stub_trade);

        assert_eq!(indicator.count, 1);
        assert_eq!(indicator.value, 1.0);
    }

    #[rstest]
    fn test_handle_bar_uses_close_and_volume() {
        let mut indicator = Vpin::new(100_000.0, 1, Some(2));
        for close in ["1500", "1510", "1500"] {
            indicator.handle_bar(&bar_ethusdt_binance_minute_bid(close));
        }

        // Same changes (+10, -10) as the bulk classification test, scaled: imbalance erf(1/2)
        assert_eq!(indicator.count, 1);
        assert!((indicator.value - 0.520_499_877_813_046_5).abs() < 1e-14);
    }

    #[rstest]
    fn test_quote_is_ignored(stub_quote: QuoteTick) {
        let mut indicator = Vpin::new(10.0, 1, None);
        let result = indicator.handle_quote(&stub_quote);

        assert!(result.is_ok());
        assert!(!indicator.has_inputs());
    }

    #[rstest]
    fn test_reset_restores_fresh_state() {
        let mut indicator = Vpin::new(10.0, 2, Some(2));
        feed_sides(
            &mut indicator,
            &[(AggressorSide::Buy, 6.0), (AggressorSide::Sell, 14.0)],
        );
        indicator.update_bar(100.0, 3.0);
        indicator.reset();

        assert_eq!(indicator.value, 0.0);
        assert_eq!(indicator.count, 0);
        assert!(!indicator.initialized());
        assert!(!indicator.has_inputs());

        // The first unsigned trade after a reset has no tick history
        indicator.update_trade(100.0, 10.0, AggressorSide::NoAggressor);
        assert_eq!(indicator.count, 0);

        feed_sides(
            &mut indicator,
            &[(AggressorSide::Buy, 6.0), (AggressorSide::Sell, 14.0)],
        );
        assert_eq!(indicator.count, 2);
        assert!((indicator.value - 0.6).abs() < TOLERANCE);
    }

    #[rstest]
    #[case(0.0, 0.5)]
    #[case(1.0, 0.841_344_746_068_542_9)]
    #[case(-1.959_963_984_540_054, 0.025)]
    #[case(-8.0, 6.220_960_574_271_784e-16)]
    fn test_normal_cdf_reference_values(#[case] x: f64, #[case] expected: f64) {
        assert!((normal_cdf(x) - expected).abs() < 1e-14);
    }

    proptest! {
        #[rstest]
        fn prop_value_is_invariant_to_splitting_trades(
            bucket_units in 1u32..=8,
            window in 1usize..=6,
            trades in prop::collection::vec((any::<bool>(), 1u32..=12), 1..=64),
        ) {
            // Integer sizes keep every bucket boundary exact, so the split stream
            // must give the same value as the whole trades
            let mut whole = Vpin::new(f64::from(bucket_units), window, None);
            let mut split = Vpin::new(f64::from(bucket_units), window, None);

            for &(is_buy, size) in &trades {
                let side = if is_buy { AggressorSide::Buy } else { AggressorSide::Sell };
                whole.update_trade(100.0, f64::from(size), side);
                for _ in 0..size {
                    split.update_trade(100.0, 1.0, side);
                }
            }

            prop_assert_eq!(whole.count, split.count);
            prop_assert_eq!(whole.initialized, split.initialized);
            prop_assert!((whole.value - split.value).abs() < TOLERANCE);
            prop_assert!((0.0..=1.0).contains(&whole.value));
        }
    }
}

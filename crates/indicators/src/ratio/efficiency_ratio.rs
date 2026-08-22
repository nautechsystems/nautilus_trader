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

use std::fmt::Display;

use anyhow::Context;
use nautilus_core::correctness::{FAILED, check_predicate_true};
use nautilus_model::{
    data::{Bar, QuoteTick, TradeTick},
    enums::PriceType,
};

use crate::indicator::Indicator;

/// Calculates Kaufman's Efficiency Ratio (ER) across a rolling window.
///
/// The period must be at least `2`.
///
/// For period `n`, the ratio is:
///
/// `ER(t) = |P(t) - P(t - n)| / sum(|P(i) - P(i - 1)|, i = t - n + 1 to t)`
///
/// A full `n`‑period window requires `n + 1` prices for `n` price changes. For
/// finite inputs within the model price range, values range from `0.0` to `1.0`:
/// lower values indicate more noise, while `1.0` indicates directional price
/// movement without reversals.
///
/// For compatibility, `initialized` becomes true after `n` inputs, so the first
/// initialized value covers the `n - 1` available price changes.
///
/// # References
///
/// - Kaufman, P. J. (1995). *Smarter Trading*. McGraw‑Hill.
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
pub struct EfficiencyRatio {
    /// The rolling window period for the indicator (>= 2).
    pub period: usize,
    pub price_type: PriceType,
    pub value: f64,
    pub inputs: Vec<f64>,
    pub initialized: bool,
    deltas: Vec<f64>,
}

impl Display for EfficiencyRatio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", self.name(), self.period)
    }
}

impl Indicator for EfficiencyRatio {
    fn name(&self) -> String {
        stringify!(EfficiencyRatio).to_string()
    }

    fn has_inputs(&self) -> bool {
        !self.inputs.is_empty()
    }
    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        self.update_raw(quote.extract_price(self.price_type)?.into());
        Ok(())
    }

    fn handle_trade(&mut self, trade: &TradeTick) {
        self.update_raw((&trade.price).into());
    }

    fn handle_bar(&mut self, bar: &Bar) {
        self.update_raw((&bar.close).into());
    }

    fn reset(&mut self) {
        self.value = 0.0;
        self.inputs.clear();
        self.deltas.clear();
        self.initialized = false;
    }
}

impl EfficiencyRatio {
    /// Creates a new [`EfficiencyRatio`] instance.
    ///
    /// # Panics
    ///
    /// Panics if `period` is less than 2 or storage for its rolling windows cannot be reserved.
    #[must_use]
    pub fn new(period: usize, price_type: Option<PriceType>) -> Self {
        Self::new_checked(period, price_type).expect(FAILED)
    }

    pub(crate) fn new_checked(
        period: usize,
        price_type: Option<PriceType>,
    ) -> anyhow::Result<Self> {
        check_predicate_true(period >= 2, "`period` must be at least 2")?;
        check_predicate_true(
            period < usize::MAX,
            "`period` must be less than `usize::MAX`",
        )?;

        let mut inputs = Vec::new();
        inputs
            .try_reserve_exact(Self::input_capacity(period))
            .context("failed to reserve efficiency ratio input window")?;

        let mut deltas = Vec::new();
        deltas
            .try_reserve_exact(period)
            .context("failed to reserve efficiency ratio delta window")?;

        Ok(Self {
            period,
            price_type: price_type.unwrap_or(PriceType::Last),
            value: 0.0,
            inputs,
            deltas,
            initialized: false,
        })
    }

    pub fn update_raw(&mut self, value: f64) {
        // A period of price changes requires one additional input
        if self.inputs.len() == Self::input_capacity(self.period) {
            self.inputs.remove(0);
        }
        self.inputs.push(value);

        if self.inputs.len() < 2 {
            self.value = 0.0;
            return;
        } else if !self.initialized && self.inputs.len() >= self.period {
            self.initialized = true;
        }
        let last_diff =
            (self.inputs[self.inputs.len() - 1] - self.inputs[self.inputs.len() - 2]).abs();
        // Bound the deltas window to `period` as well, so the sum reflects only
        // the last `period` absolute changes.
        if self.deltas.len() == self.period {
            self.deltas.remove(0);
        }
        self.deltas.push(last_diff);
        let sum_deltas = self.deltas.iter().sum::<f64>();
        let net_diff = (self.inputs[self.inputs.len() - 1] - self.inputs[0]).abs();
        self.value = if sum_deltas == 0.0 {
            0.0
        } else {
            (net_diff / sum_deltas).clamp(0.0, 1.0)
        };
    }

    const fn input_capacity(period: usize) -> usize {
        period.saturating_add(1)
    }
}

#[cfg(test)]
mod tests {

    use nautilus_model::types::{PRICE_MAX, PRICE_MIN};
    use proptest::prelude::*;
    use rstest::rstest;

    use crate::{
        indicator::Indicator, ratio::efficiency_ratio::EfficiencyRatio, stubs::*,
        testing::assert_approx_equal,
    };

    #[rstest]
    fn test_efficiency_ratio_initialized(efficiency_ratio_10: EfficiencyRatio) {
        let display_str = format!("{efficiency_ratio_10}");
        assert_eq!(display_str, "EfficiencyRatio(10)");
        assert_eq!(efficiency_ratio_10.period, 10);
        assert!(!efficiency_ratio_10.initialized);
    }

    #[rstest]
    #[case(0)]
    #[case(1)]
    #[should_panic(expected = "`period` must be at least 2")]
    fn test_new_rejects_period_below_two(#[case] period: usize) {
        let _ = EfficiencyRatio::new(period, None);
    }

    #[rstest]
    #[case(usize::MAX, "`period` must be less than `usize::MAX`")]
    #[case(
        usize::MAX - 1,
        "failed to reserve efficiency ratio input window"
    )]
    fn test_new_checked_rejects_unrepresentable_input_window(
        #[case] period: usize,
        #[case] expected: &str,
    ) {
        let error = EfficiencyRatio::new_checked(period, None).unwrap_err();

        assert_eq!(error.to_string(), expected);
    }

    #[rstest]
    fn test_with_correct_number_of_required_inputs(mut efficiency_ratio_10: EfficiencyRatio) {
        for i in 1..10 {
            efficiency_ratio_10.update_raw(f64::from(i));
        }
        assert_eq!(efficiency_ratio_10.inputs.len(), 9);
        assert!(!efficiency_ratio_10.initialized);
        efficiency_ratio_10.update_raw(1.0);
        assert_eq!(efficiency_ratio_10.inputs.len(), 10);
        assert!(efficiency_ratio_10.initialized);
    }

    #[rstest]
    fn test_value_with_one_input(mut efficiency_ratio_10: EfficiencyRatio) {
        efficiency_ratio_10.update_raw(1.0);
        assert_eq!(efficiency_ratio_10.value, 0.0);
    }

    #[rstest]
    fn test_value_with_efficient_higher_inputs(mut efficiency_ratio_10: EfficiencyRatio) {
        let mut initial_price = 1.0;
        for _ in 1..=10 {
            initial_price += 0.0001;
            efficiency_ratio_10.update_raw(initial_price);
        }
        assert_eq!(efficiency_ratio_10.value, 1.0);
    }

    #[rstest]
    fn test_value_with_efficient_lower_inputs(mut efficiency_ratio_10: EfficiencyRatio) {
        let mut initial_price = 1.0;
        for _ in 1..=10 {
            initial_price -= 0.0001;
            efficiency_ratio_10.update_raw(initial_price);
        }
        assert_eq!(efficiency_ratio_10.value, 1.0);
    }

    #[rstest]
    fn test_value_with_oscillating_inputs_returns_zero(mut efficiency_ratio_10: EfficiencyRatio) {
        efficiency_ratio_10.update_raw(1.00000);
        efficiency_ratio_10.update_raw(1.00010);
        efficiency_ratio_10.update_raw(1.00000);
        efficiency_ratio_10.update_raw(0.99990);
        efficiency_ratio_10.update_raw(1.00000);
        assert_eq!(efficiency_ratio_10.value, 0.0);
    }

    #[rstest]
    fn test_value_with_half_oscillating(mut efficiency_ratio_10: EfficiencyRatio) {
        efficiency_ratio_10.update_raw(1.00000);
        efficiency_ratio_10.update_raw(1.00020);
        efficiency_ratio_10.update_raw(1.00010);
        efficiency_ratio_10.update_raw(1.00030);
        efficiency_ratio_10.update_raw(1.00020);
        assert_approx_equal(efficiency_ratio_10.value, 0.333333333333);
    }

    #[rstest]
    fn test_value_with_noisy_inputs(mut efficiency_ratio_10: EfficiencyRatio) {
        efficiency_ratio_10.update_raw(1.00000);
        efficiency_ratio_10.update_raw(1.00010);
        efficiency_ratio_10.update_raw(1.00008);
        efficiency_ratio_10.update_raw(1.00007);
        efficiency_ratio_10.update_raw(1.00012);
        efficiency_ratio_10.update_raw(1.00005);
        efficiency_ratio_10.update_raw(1.00015);
        assert_approx_equal(efficiency_ratio_10.value, 0.428571428572);
    }

    #[rstest]
    #[case([10.0, 11.0, 12.0, 13.0, 14.0, 15.0], 1.0)]
    #[case([15.0, 14.0, 13.0, 12.0, 11.0, 10.0], 1.0)]
    #[case([10.0, 12.0, 10.0, 12.0, 10.0, 12.0], 0.2)]
    #[case([10.0, 11.0, 10.5, 12.0, 11.5, 13.0], 0.6)]
    #[case([10.0, 10.0, 10.0, 10.0, 10.0, 10.0], 0.0)]
    fn test_value_uses_period_deltas(#[case] prices: [f64; 6], #[case] expected: f64) {
        let mut efficiency_ratio = EfficiencyRatio::new(5, None);

        for price in prices {
            efficiency_ratio.update_raw(price);
        }

        assert_approx_equal(efficiency_ratio.value, expected);
    }

    #[rstest]
    fn test_value_bounded_after_warmup() {
        let mut efficiency_ratio = EfficiencyRatio::new(5, None);

        for price in [10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 14.0] {
            efficiency_ratio.update_raw(price);
        }

        assert_eq!(
            efficiency_ratio.inputs,
            vec![11.0, 12.0, 13.0, 14.0, 15.0, 14.0],
        );
        assert_eq!(efficiency_ratio.deltas, vec![1.0; 5]);
        assert_approx_equal(efficiency_ratio.value, 0.6);
    }

    #[rstest]
    fn test_update_raw_reuses_reserved_window_storage() {
        let mut efficiency_ratio = EfficiencyRatio::new(5, None);
        let inputs_capacity = efficiency_ratio.inputs.capacity();
        let deltas_capacity = efficiency_ratio.deltas.capacity();

        for price in 0..1_000 {
            efficiency_ratio.update_raw(f64::from(price));
        }

        assert_eq!(efficiency_ratio.inputs.capacity(), inputs_capacity);
        assert_eq!(efficiency_ratio.deltas.capacity(), deltas_capacity);
        assert_eq!(efficiency_ratio.inputs.len(), 6);
        assert_eq!(efficiency_ratio.deltas.len(), 5);
    }

    #[rstest]
    fn test_value_clamps_rounding_above_one() {
        let mut efficiency_ratio = EfficiencyRatio::new(2, None);

        for price in [0.0, 0.002, 101_070.264] {
            efficiency_ratio.update_raw(price);
        }

        assert_eq!(efficiency_ratio.value, 1.0);
    }

    #[rstest]
    fn test_value_remains_bounded_at_price_limits() {
        let period = 63;
        let mut efficiency_ratio = EfficiencyRatio::new(period, None);

        for index in 0..512 {
            let price = if index % 2 == 0 { PRICE_MIN } else { PRICE_MAX };
            efficiency_ratio.update_raw(price);

            assert!(efficiency_ratio.value.is_finite());
            assert!((0.0..=1.0).contains(&efficiency_ratio.value));
        }

        assert_approx_equal(efficiency_ratio.value, 1.0 / period as f64);
    }

    proptest! {
        #[rstest]
        fn prop_value_matches_fixed_point_reference(
            period in 2usize..=64,
            prices in prop::collection::vec(-1_000_000_000i64..=1_000_000_000, 257..=512),
        ) {
            let mut efficiency_ratio = EfficiencyRatio::new(period, None);

            for (index, raw_price) in prices.iter().copied().enumerate() {
                efficiency_ratio.update_raw(raw_price as f64 / 1_000.0);

                let expected = reference_value(&prices[..=index], period);
                let error = (efficiency_ratio.value - expected).abs();

                prop_assert!(efficiency_ratio.value.is_finite());
                prop_assert!((0.0..=1.0).contains(&efficiency_ratio.value));
                prop_assert!(
                    error <= 1e-12,
                    "expected {expected}, was {}",
                    efficiency_ratio.value,
                );
                prop_assert_eq!(
                    efficiency_ratio.inputs.len(),
                    (index + 1).min(period + 1),
                );
                prop_assert_eq!(efficiency_ratio.deltas.len(), index.min(period));
                prop_assert_eq!(efficiency_ratio.initialized, index + 1 >= period);
            }
        }
    }

    fn reference_value(prices: &[i64], period: usize) -> f64 {
        let window = &prices[prices.len().saturating_sub(period + 1)..];
        let net_change = (i128::from(window[window.len() - 1]) - i128::from(window[0])).abs();
        let total_change = window
            .windows(2)
            .map(|pair| (i128::from(pair[1]) - i128::from(pair[0])).abs())
            .sum::<i128>();

        if total_change == 0 {
            0.0
        } else {
            net_change as f64 / total_change as f64
        }
    }

    #[rstest]
    fn test_reset_clears_deltas(mut efficiency_ratio_10: EfficiencyRatio) {
        // Regression: reset must clear the deltas buffer too, otherwise stale
        // deltas leak into the next run's sum.
        for price in [1.0, 3.0, 6.0, 10.0, 15.0] {
            efficiency_ratio_10.update_raw(price);
        }
        efficiency_ratio_10.reset();
        assert!(efficiency_ratio_10.deltas.is_empty());

        // Fresh run: two inputs of a single clean move give a ratio of 1.
        efficiency_ratio_10.update_raw(100.0);
        efficiency_ratio_10.update_raw(100.5);
        assert_eq!(efficiency_ratio_10.value, 1.0);
    }

    #[rstest]
    fn test_reset(mut efficiency_ratio_10: EfficiencyRatio) {
        for i in 1..=10 {
            efficiency_ratio_10.update_raw(f64::from(i));
        }
        assert!(efficiency_ratio_10.initialized);
        efficiency_ratio_10.reset();
        assert!(!efficiency_ratio_10.initialized);
        assert_eq!(efficiency_ratio_10.value, 0.0);
    }

    #[rstest]
    fn test_handle_quote_tick(mut efficiency_ratio_10: EfficiencyRatio) {
        let quote_tick1 = stub_quote("1500.0", "1502.0");
        let quote_tick2 = stub_quote("1502.0", "1504.0");

        efficiency_ratio_10.handle_quote(&quote_tick1).unwrap();
        efficiency_ratio_10.handle_quote(&quote_tick2).unwrap();
        assert_eq!(efficiency_ratio_10.value, 1.0);
    }

    #[rstest]
    fn test_handle_bar(mut efficiency_ratio_10: EfficiencyRatio) {
        let bar1 = bar_ethusdt_binance_minute_bid("1500.0");
        let bar2 = bar_ethusdt_binance_minute_bid("1510.0");

        efficiency_ratio_10.handle_bar(&bar1);
        efficiency_ratio_10.handle_bar(&bar2);
        assert_eq!(efficiency_ratio_10.value, 1.0);
    }
}

// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{collections::VecDeque, fmt::Display};

use nautilus_core::correctness::{FAILED, check_predicate_true};
use nautilus_model::{
    data::{Bar, QuoteTick, TradeTick},
    enums::PriceType,
};

use crate::indicator::{Indicator, MovingAverage};

/// An indicator which calculates a weighted moving average across a rolling window.
#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators")
)]
pub struct WeightedMovingAverage {
    /// The rolling window period for the indicator (> 0).
    pub period: usize,
    /// The weights for the moving average calculation
    pub weights: Vec<f64>,
    /// Price type
    pub price_type: PriceType,
    /// The last indicator value.
    pub value: f64,
    /// Whether the indicator is initialized.
    pub initialized: bool,
    /// Inputs
    pub inputs: VecDeque<f64>,
}

impl Display for WeightedMovingAverage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({},{:?})", self.name(), self.period, self.weights)
    }
}

impl WeightedMovingAverage {
    /// Creates a new [`WeightedMovingAverage`] instance.
    ///
    /// # Panics
    ///
    /// This function panics if:
    /// - `period` is zero.
    /// - `weights.len()` does not equal `period`.
    /// - `weights` sum is effectively zero.
    #[must_use]
    pub fn new(period: usize, weights: Vec<f64>, price_type: Option<PriceType>) -> Self {
        Self::new_checked(period, weights, price_type).expect(FAILED)
    }

    /// Creates a new [`WeightedMovingAverage`] instance with the given period and weights.
    ///
    /// # Errors
    ///
    /// Returns an erro if **any** of the validation rules fails:
    /// - `period` must be **positive**.
    /// - `weights` must be **exactly** `period` elements long.
    /// - `weights` must contain at least one non-zero value (∑wᵢ > ε).
    pub fn new_checked(
        period: usize,
        weights: Vec<f64>,
        price_type: Option<PriceType>,
    ) -> anyhow::Result<Self> {
        const EPS: f64 = f64::EPSILON; // ≈ 2.22 e-16

        check_predicate_true(period > 0, "`period` must be positive")?;

        check_predicate_true(
            period == weights.len(),
            "`period` must equal `weights.len()`",
        )?;

        let weight_sum: f64 = weights.iter().copied().sum();
        check_predicate_true(
            weight_sum > EPS,
            "`weights` sum must be positive and > f64::EPSILON",
        )?;

        Ok(Self {
            period,
            weights,
            price_type: price_type.unwrap_or(PriceType::Last),
            value: 0.0,
            inputs: VecDeque::with_capacity(period),
            initialized: false,
        })
    }

    fn weighted_average(&self) -> f64 {
        let n = self.inputs.len();
        let weights_slice = &self.weights[self.period - n..];

        let mut sum = 0.0;
        let mut weight_sum = 0.0;

        for (input, weight) in self.inputs.iter().rev().zip(weights_slice.iter().rev()) {
            sum += input * weight;
            weight_sum += weight;
        }
        sum / weight_sum
    }
}

impl Indicator for WeightedMovingAverage {
    fn name(&self) -> String {
        stringify!(WeightedMovingAverage).to_string()
    }

    fn has_inputs(&self) -> bool {
        !self.inputs.is_empty()
    }

    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_quote(&mut self, quote: &QuoteTick) {
        self.update_raw(quote.extract_price(self.price_type).into());
    }

    fn handle_trade(&mut self, trade: &TradeTick) {
        self.update_raw((&trade.price).into());
    }

    fn handle_bar(&mut self, bar: &Bar) {
        self.update_raw((&bar.close).into());
    }

    fn reset(&mut self) {
        self.value = 0.0;
        self.initialized = false;
        self.inputs.clear();
    }
}

impl MovingAverage for WeightedMovingAverage {
    fn value(&self) -> f64 {
        self.value
    }

    fn count(&self) -> usize {
        self.inputs.len()
    }

    fn update_raw(&mut self, value: f64) {
        if self.inputs.len() == self.period {
            self.inputs.pop_front();
        }
        self.inputs.push_back(value);

        self.value = self.weighted_average();

        self.initialized = self.count() >= self.period;
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        f64::{INFINITY, NAN},
    };

    use rstest::rstest;

    use crate::{
        average::wma::WeightedMovingAverage,
        indicator::{Indicator, MovingAverage},
        stubs::*,
    };

    #[rstest]
    fn test_wma_initialized(indicator_wma_10: WeightedMovingAverage) {
        let display_str = format!("{indicator_wma_10}");
        assert_eq!(
            display_str,
            "WeightedMovingAverage(10,[0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0])"
        );
        assert_eq!(indicator_wma_10.name(), "WeightedMovingAverage");
        assert!(!indicator_wma_10.has_inputs());
        assert!(!indicator_wma_10.initialized());
    }

    #[rstest]
    #[should_panic]
    fn test_different_weights_len_and_period_error() {
        let _ = WeightedMovingAverage::new(10, vec![0.5, 0.5, 0.5], None);
    }

    #[rstest]
    fn test_value_with_one_input(mut indicator_wma_10: WeightedMovingAverage) {
        indicator_wma_10.update_raw(1.0);
        assert_eq!(indicator_wma_10.value, 1.0);
    }

    #[rstest]
    fn test_value_with_two_inputs_equal_weights() {
        let mut wma = WeightedMovingAverage::new(2, vec![0.5, 0.5], None);
        wma.update_raw(1.0);
        wma.update_raw(2.0);
        assert_eq!(wma.value, 1.5);
    }

    #[rstest]
    fn test_value_with_four_inputs_equal_weights() {
        let mut wma = WeightedMovingAverage::new(4, vec![0.25, 0.25, 0.25, 0.25], None);
        wma.update_raw(1.0);
        wma.update_raw(2.0);
        wma.update_raw(3.0);
        wma.update_raw(4.0);
        assert_eq!(wma.value, 2.5);
    }

    #[rstest]
    fn test_value_with_two_inputs(mut indicator_wma_10: WeightedMovingAverage) {
        indicator_wma_10.update_raw(1.0);
        indicator_wma_10.update_raw(2.0);
        let result = 2.0f64.mul_add(1.0, 1.0 * 0.9) / 1.9;
        assert_eq!(indicator_wma_10.value, result);
    }

    #[rstest]
    fn test_value_with_three_inputs(mut indicator_wma_10: WeightedMovingAverage) {
        indicator_wma_10.update_raw(1.0);
        indicator_wma_10.update_raw(2.0);
        indicator_wma_10.update_raw(3.0);
        let result = 1.0f64.mul_add(0.8, 3.0f64.mul_add(1.0, 2.0 * 0.9)) / (1.0 + 0.9 + 0.8);
        assert_eq!(indicator_wma_10.value, result);
    }

    #[rstest]
    fn test_value_expected_with_exact_period(mut indicator_wma_10: WeightedMovingAverage) {
        for i in 1..11 {
            indicator_wma_10.update_raw(f64::from(i));
        }
        assert_eq!(indicator_wma_10.value, 7.0);
    }

    #[rstest]
    fn test_value_expected_with_more_inputs(mut indicator_wma_10: WeightedMovingAverage) {
        for i in 1..=11 {
            indicator_wma_10.update_raw(f64::from(i));
        }
        assert_eq!(indicator_wma_10.value(), 8.000_000_000_000_002);
    }

    #[rstest]
    fn test_reset(mut indicator_wma_10: WeightedMovingAverage) {
        indicator_wma_10.update_raw(1.0);
        indicator_wma_10.update_raw(2.0);
        indicator_wma_10.reset();
        assert_eq!(indicator_wma_10.value, 0.0);
        assert_eq!(indicator_wma_10.count(), 0);
        assert!(!indicator_wma_10.initialized);
    }

    #[rstest]
    #[should_panic]
    fn new_panics_on_zero_period() {
        let _ = WeightedMovingAverage::new(0, vec![1.0], None);
    }

    #[rstest]
    fn new_checked_err_on_zero_period() {
        let res = WeightedMovingAverage::new_checked(0, vec![1.0], None);
        assert!(res.is_err());
    }

    #[rstest]
    #[should_panic]
    fn new_panics_on_zero_weight_sum() {
        let _ = WeightedMovingAverage::new(3, vec![0.0, 0.0, 0.0], None);
    }

    #[rstest]
    fn new_checked_err_on_zero_weight_sum() {
        let res = WeightedMovingAverage::new_checked(3, vec![0.0, 0.0, 0.0], None);
        assert!(res.is_err());
    }

    #[rstest]
    #[should_panic]
    fn new_panics_when_weight_sum_below_epsilon() {
        let tiny = f64::EPSILON / 10.0;
        let _ = WeightedMovingAverage::new(3, vec![tiny; 3], None);
    }

    #[rstest]
    fn initialized_flag_transitions() {
        let period = 3;
        let weights = vec![1.0, 2.0, 3.0];
        let mut wma = WeightedMovingAverage::new(period, weights, None);

        assert!(!wma.initialized());

        for i in 0..period {
            wma.update_raw(i as f64);
            let expected = (i + 1) >= period;
            assert_eq!(wma.initialized(), expected);
        }
        assert!(wma.initialized());
    }

    #[rstest]
    fn count_matches_inputs_and_has_inputs() {
        let mut wma = WeightedMovingAverage::new(4, vec![0.25; 4], None);

        assert_eq!(wma.count(), 0);
        assert!(!wma.has_inputs());

        wma.update_raw(1.0);
        wma.update_raw(2.0);
        assert_eq!(wma.count(), 2);
        assert!(wma.has_inputs());
    }

    #[rstest]
    fn reset_restores_pristine_state() {
        let mut wma = WeightedMovingAverage::new(2, vec![0.5, 0.5], None);
        wma.update_raw(1.0);
        wma.update_raw(2.0);
        assert!(wma.initialized());

        wma.reset();

        assert_eq!(wma.count(), 0);
        assert_eq!(wma.value(), 0.0);
        assert!(!wma.initialized());
        assert!(!wma.has_inputs());
    }

    #[rstest]
    fn weighted_average_with_non_uniform_weights() {
        let mut wma = WeightedMovingAverage::new(3, vec![1.0, 2.0, 3.0], None);
        wma.update_raw(10.0);
        wma.update_raw(20.0);
        wma.update_raw(30.0);
        let expected = 23.333_333_333_333_332;
        let tol = f64::EPSILON.sqrt();
        assert!(
            (wma.value() - expected).abs() < tol,
            "value = {}, expected ≈ {}",
            wma.value(),
            expected
        );
    }

    #[rstest]
    fn test_window_never_exceeds_period(mut indicator_wma_10: WeightedMovingAverage) {
        for i in 0..100 {
            indicator_wma_10.update_raw(f64::from(i));
            assert!(indicator_wma_10.count() <= indicator_wma_10.period);
        }
    }

    #[rstest]
    fn test_negative_weights_positive_sum() {
        let period = 3;
        let weights = vec![-1.0, 2.0, 2.0];
        let mut wma = WeightedMovingAverage::new(period, weights, None);
        wma.update_raw(1.0);
        wma.update_raw(2.0);
        wma.update_raw(3.0);

        let expected = 2.0f64.mul_add(3.0, 2.0f64.mul_add(2.0, -1.0)) / 3.0;
        let tol = f64::EPSILON.sqrt();
        assert!((wma.value() - expected).abs() < tol);
    }

    #[rstest]
    fn test_nan_input_propagates() {
        use std::f64::NAN;

        let mut wma = WeightedMovingAverage::new(2, vec![0.5, 0.5], None);
        wma.update_raw(1.0);
        wma.update_raw(NAN);

        assert!(wma.value().is_nan());
    }

    #[rstest]
    #[should_panic]
    fn new_panics_when_weight_sum_equals_epsilon() {
        let eps_third = f64::EPSILON / 3.0;
        let _ = WeightedMovingAverage::new(3, vec![eps_third; 3], None);
    }

    #[rstest]
    fn new_checked_err_when_weight_sum_equals_epsilon() {
        let eps_third = f64::EPSILON / 3.0;
        let res = WeightedMovingAverage::new_checked(3, vec![eps_third; 3], None);
        assert!(res.is_err());
    }

    #[rstest]
    fn new_checked_err_when_weight_sum_below_epsilon() {
        let w = f64::EPSILON * 0.9;
        let res = WeightedMovingAverage::new_checked(1, vec![w], None);
        assert!(res.is_err());
    }

    #[rstest]
    fn new_ok_when_weight_sum_above_epsilon() {
        let w = f64::EPSILON * 1.1;
        let res = WeightedMovingAverage::new_checked(1, vec![w], None);
        assert!(res.is_ok());
    }

    #[rstest]
    #[should_panic]
    fn new_panics_on_cancelled_weights_sum() {
        let _ = WeightedMovingAverage::new(3, vec![1.0, -1.0, 0.0], None);
    }

    #[rstest]
    fn new_checked_err_on_cancelled_weights_sum() {
        let res = WeightedMovingAverage::new_checked(3, vec![1.0, -1.0, 0.0], None);
        assert!(res.is_err());
    }

    #[rstest]
    fn single_period_returns_latest_input() {
        let mut wma = WeightedMovingAverage::new(1, vec![1.0], None);
        for i in 0..5 {
            let v = f64::from(i);
            wma.update_raw(v);
            assert_eq!(wma.value(), v);
        }
    }

    #[rstest]
    fn value_with_sparse_weights() {
        let mut wma = WeightedMovingAverage::new(3, vec![0.0, 1.0, 0.0], None);
        wma.update_raw(10.0);
        wma.update_raw(20.0);
        wma.update_raw(30.0);
        assert_eq!(wma.value(), 20.0);
    }

    #[rstest]
    fn warm_up_len1() {
        let mut wma = WeightedMovingAverage::new(4, vec![1.0, 2.0, 3.0, 4.0], None);
        wma.update_raw(42.0);
        assert_eq!(wma.value(), 42.0);
    }

    #[rstest]
    fn warm_up_len2() {
        let mut wma = WeightedMovingAverage::new(4, vec![1.0, 2.0, 3.0, 4.0], None);
        wma.update_raw(10.0);
        wma.update_raw(20.0);
        let expected = 20.0f64.mul_add(4.0, 10.0 * 3.0) / (4.0 + 3.0);
        assert_eq!(wma.value(), expected);
    }

    #[rstest]
    fn warm_up_len3() {
        let mut wma = WeightedMovingAverage::new(4, vec![1.0, 2.0, 3.0, 4.0], None);
        wma.update_raw(1.0);
        wma.update_raw(2.0);
        wma.update_raw(3.0);
        let expected = 1.0f64.mul_add(2.0, 3.0f64.mul_add(4.0, 2.0 * 3.0)) / (4.0 + 3.0 + 2.0);
        assert_eq!(wma.value(), expected);
    }

    #[rstest]
    fn input_window_contains_latest_period() {
        let period = 3;
        let mut wma = WeightedMovingAverage::new(period, vec![1.0; period], None);
        let vals = [1.0, 2.0, 3.0, 4.0];
        for v in vals {
            wma.update_raw(v);
        }
        let expected: Vec<f64> = vals[vals.len() - period..].to_vec();
        assert_eq!(wma.inputs.iter().copied().collect::<Vec<_>>(), expected);
    }

    #[rstest]
    fn window_slides_correctly() {
        let mut wma = WeightedMovingAverage::new(2, vec![1.0; 2], None);
        wma.update_raw(1.0);
        assert_eq!(wma.inputs.iter().copied().collect::<Vec<_>>(), vec![1.0]);
        wma.update_raw(2.0);
        assert_eq!(
            wma.inputs.iter().copied().collect::<Vec<_>>(),
            vec![1.0, 2.0]
        );
        wma.update_raw(3.0);
        assert_eq!(
            wma.inputs.iter().copied().collect::<Vec<_>>(),
            vec![2.0, 3.0]
        );
    }

    #[rstest]
    fn window_len_constant_after_many_updates() {
        let period = 5;
        let mut wma = WeightedMovingAverage::new(period, vec![1.0; period], None);
        for i in 0..100 {
            wma.update_raw(i as f64);
            assert_eq!(wma.inputs.len(), period.min(i + 1));
        }
    }

    #[rstest]
    #[should_panic]
    fn new_panics_on_nan_weight() {
        let _ = WeightedMovingAverage::new(2, vec![NAN, 1.0], None);
    }

    #[rstest]
    fn new_ok_with_infinite_weight() {
        let res = WeightedMovingAverage::new_checked(2, vec![INFINITY, 1.0], None);
        assert!(res.is_ok());
    }

    #[rstest]
    #[should_panic]
    fn new_panics_on_empty_weights() {
        let _ = WeightedMovingAverage::new(1, Vec::new(), None);
    }

    #[rstest]
    fn inf_input_propagates() {
        let mut wma = WeightedMovingAverage::new(2, vec![0.5, 0.5], None);
        wma.update_raw(1.0);
        wma.update_raw(INFINITY);
        assert!(wma.value().is_infinite());
    }

    #[rstest]
    fn warm_up_with_front_zero_weights() {
        let mut wma = WeightedMovingAverage::new(4, vec![0.0, 0.0, 1.0, 1.0], None);
        wma.update_raw(10.0);
        wma.update_raw(20.0);
        let expected = 20.0f64.mul_add(1.0, 10.0 * 1.0) / 2.0;
        assert_eq!(wma.value(), expected);
    }

    #[rstest]
    fn vecdeque_grows_without_pop() {
        let period = 3;
        let mut buf: VecDeque<usize> = VecDeque::with_capacity(period);
        for i in 0..=period {
            buf.push_back(i);
        }
        assert_eq!(buf.len(), period + 1);
    }

    #[rstest]
    fn vecdeque_sliding_window_with_pop() {
        let period = 3;
        let mut buf: VecDeque<usize> = VecDeque::with_capacity(period);
        for i in 0..10 {
            if buf.len() == period {
                buf.pop_front();
            }
            buf.push_back(i);
            assert!(buf.len() <= period);
        }
        assert_eq!(buf.len(), period);
    }
}

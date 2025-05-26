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

use std::fmt::{Debug, Display};

use arraydeque::{ArrayDeque, Wrapping};
use nautilus_model::data::Bar;

use crate::indicator::Indicator;

const MAX_PERIOD: usize = 16_384;

#[repr(C)]
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators")
)]
pub struct LinearRegression {
    pub period: usize,
    pub slope: f64,
    pub intercept: f64,
    pub degree: f64,
    pub cfo: f64,
    pub r2: f64,
    pub value: f64,
    pub initialized: bool,
    has_inputs: bool,
    inputs: ArrayDeque<f64, MAX_PERIOD, Wrapping>,
    x_sum: f64,
    x_mul_sum: f64,
    divisor: f64,
}

impl Display for LinearRegression {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", self.name(), self.period)
    }
}

impl Indicator for LinearRegression {
    fn name(&self) -> String {
        stringify!(LinearRegression).into()
    }

    fn has_inputs(&self) -> bool {
        self.has_inputs
    }

    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_bar(&mut self, bar: &Bar) {
        self.update_raw(bar.close.into());
    }

    fn reset(&mut self) {
        self.slope = 0.0;
        self.intercept = 0.0;
        self.degree = 0.0;
        self.cfo = 0.0;
        self.r2 = 0.0;
        self.value = 0.0;
        self.inputs.clear();
        self.has_inputs = false;
        self.initialized = false;
    }
}

impl LinearRegression {
    /// Creates a new [`LinearRegression`] instance.
    ///
    /// # Panics
    ///
    /// This function panics if:
    /// `period` is zero.
    /// `period` exceeds [`MAX_PERIOD`].
    #[must_use]
    pub fn new(period: usize) -> Self {
        assert!(
            period > 0,
            "LinearRegression: period must be > 0 (received {period})"
        );
        assert!(
            period <= MAX_PERIOD,
            "LinearRegression: period {period} exceeds MAX_PERIOD ({MAX_PERIOD})"
        );

        let n = period as f64;
        let x_sum = 0.5 * n * (n + 1.0);
        let x_mul_sum = x_sum * 2.0f64.mul_add(n, 1.0) / 3.0;
        let divisor = n.mul_add(x_mul_sum, -(x_sum * x_sum));

        Self {
            period,
            slope: 0.0,
            intercept: 0.0,
            degree: 0.0,
            cfo: 0.0,
            r2: 0.0,
            value: 0.0,
            initialized: false,
            has_inputs: false,
            inputs: ArrayDeque::new(),
            x_sum,
            x_mul_sum,
            divisor,
        }
    }

    /// Updates the linear regression with a new data point.
    ///
    /// # Panics
    ///
    /// Panics if called with an empty window – this is protected against by the logic
    /// that returns early until enough samples have been collected.
    pub fn update_raw(&mut self, close: f64) {
        if self.inputs.len() == self.period {
            let _ = self.inputs.pop_front();
        }
        let _ = self.inputs.push_back(close);

        self.has_inputs = true;
        if self.inputs.len() < self.period {
            return;
        }
        self.initialized = true;

        let n = self.period as f64;
        let x_sum = self.x_sum;
        let x_mul_sum = self.x_mul_sum;
        let divisor = self.divisor;

        let (mut y_sum, mut xy_sum) = (0.0, 0.0);
        for (i, &y) in self.inputs.iter().enumerate() {
            let x = (i + 1) as f64;
            y_sum += y;
            xy_sum += x * y;
        }

        self.slope = n.mul_add(xy_sum, -(x_sum * y_sum)) / divisor;
        self.intercept = y_sum.mul_add(x_mul_sum, -(x_sum * xy_sum)) / divisor;

        let (mut sse, mut y_last, mut e_last) = (0.0, 0.0, 0.0);
        for (i, &y) in self.inputs.iter().enumerate() {
            let x = (i + 1) as f64;
            let y_hat = self.slope.mul_add(x, self.intercept);
            let resid = y_hat - y;
            sse += resid * resid;
            y_last = y;
            e_last = resid;
        }

        self.value = y_last + e_last;
        self.degree = self.slope.atan().to_degrees();
        self.cfo = if y_last == 0.0 {
            f64::NAN
        } else {
            100.0 * e_last / y_last
        };

        let mean = y_sum / n;
        let sst: f64 = self
            .inputs
            .iter()
            .map(|&y| {
                let d = y - mean;
                d * d
            })
            .sum();

        self.r2 = if sst.abs() < f64::EPSILON {
            f64::NAN
        } else {
            1.0 - sse / sst
        };
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_model::data::Bar;
    use rstest::rstest;

    use super::*;
    use crate::{
        average::lr::LinearRegression,
        indicator::Indicator,
        stubs::{bar_ethusdt_binance_minute_bid, indicator_lr_10},
    };

    #[rstest]
    fn test_psl_initialized(indicator_lr_10: LinearRegression) {
        let display_str = format!("{indicator_lr_10}");
        assert_eq!(display_str, "LinearRegression(10)");
        assert_eq!(indicator_lr_10.period, 10);
        assert!(!indicator_lr_10.initialized);
        assert!(!indicator_lr_10.has_inputs);
    }

    #[rstest]
    #[should_panic(expected = "LinearRegression: period must be > 0")]
    fn test_new_with_zero_period_panics() {
        let _ = LinearRegression::new(0);
    }

    #[rstest]
    fn test_value_with_one_input(mut indicator_lr_10: LinearRegression) {
        indicator_lr_10.update_raw(1.0);
        assert_eq!(indicator_lr_10.value, 0.0);
    }

    #[rstest]
    fn test_value_with_three_inputs(mut indicator_lr_10: LinearRegression) {
        indicator_lr_10.update_raw(1.0);
        indicator_lr_10.update_raw(2.0);
        indicator_lr_10.update_raw(3.0);
        assert_eq!(indicator_lr_10.value, 0.0);
    }

    #[rstest]
    fn test_initialized_with_required_input(mut indicator_lr_10: LinearRegression) {
        for i in 1..10 {
            indicator_lr_10.update_raw(f64::from(i));
        }
        assert!(!indicator_lr_10.initialized);
        indicator_lr_10.update_raw(10.0);
        assert!(indicator_lr_10.initialized);
    }

    #[rstest]
    fn test_handle_bar(mut indicator_lr_10: LinearRegression, bar_ethusdt_binance_minute_bid: Bar) {
        indicator_lr_10.handle_bar(&bar_ethusdt_binance_minute_bid);
        assert_eq!(indicator_lr_10.value, 0.0);
        assert!(indicator_lr_10.has_inputs);
        assert!(!indicator_lr_10.initialized);
    }

    #[rstest]
    fn test_reset(mut indicator_lr_10: LinearRegression) {
        indicator_lr_10.update_raw(1.0);
        indicator_lr_10.reset();
        assert_eq!(indicator_lr_10.value, 0.0);
        assert_eq!(indicator_lr_10.inputs.len(), 0);
        assert_eq!(indicator_lr_10.slope, 0.0);
        assert_eq!(indicator_lr_10.intercept, 0.0);
        assert_eq!(indicator_lr_10.degree, 0.0);
        assert_eq!(indicator_lr_10.cfo, 0.0);
        assert_eq!(indicator_lr_10.r2, 0.0);
        assert!(!indicator_lr_10.has_inputs);
        assert!(!indicator_lr_10.initialized);
    }

    #[rstest]
    fn test_inputs_len_never_exceeds_period() {
        let mut lr = LinearRegression::new(3);
        for i in 0..10 {
            lr.update_raw(f64::from(i));
        }
        assert_eq!(lr.inputs.len(), lr.period);
    }

    #[rstest]
    fn test_oldest_element_evicted() {
        let mut lr = LinearRegression::new(4);
        for v in 1..=5 {
            lr.update_raw(f64::from(v));
        }
        assert!(!lr.inputs.contains(&1.0));
        assert_eq!(lr.inputs.front(), Some(&2.0));
    }

    #[rstest]
    fn test_recent_elements_preserved() {
        let mut lr = LinearRegression::new(5);
        for v in 0..5 {
            lr.update_raw(f64::from(v));
        }
        lr.update_raw(99.0);
        let expected = vec![1.0, 2.0, 3.0, 4.0, 99.0];
        assert_eq!(lr.inputs.iter().copied().collect::<Vec<_>>(), expected);
    }

    #[rstest]
    fn test_multiple_evictions() {
        let mut lr = LinearRegression::new(2);
        lr.update_raw(10.0);
        lr.update_raw(20.0);
        lr.update_raw(30.0);
        lr.update_raw(40.0);
        assert_eq!(
            lr.inputs.iter().copied().collect::<Vec<_>>(),
            vec![30.0, 40.0]
        );
    }

    #[rstest]
    fn test_value_stable_after_eviction() {
        let mut lr = LinearRegression::new(3);
        lr.update_raw(1.0);
        lr.update_raw(2.0);
        lr.update_raw(3.0);
        let before = lr.value;
        lr.update_raw(4.0);
        let after = lr.value;
        assert!(after.is_finite());
        assert_ne!(before, after);
    }

    #[rstest]
    fn test_value_with_ten_inputs(mut indicator_lr_10: LinearRegression) {
        indicator_lr_10.update_raw(1.00000);
        indicator_lr_10.update_raw(1.00010);
        indicator_lr_10.update_raw(1.00030);
        indicator_lr_10.update_raw(1.00040);
        indicator_lr_10.update_raw(1.00050);
        indicator_lr_10.update_raw(1.00060);
        indicator_lr_10.update_raw(1.00050);
        indicator_lr_10.update_raw(1.00040);
        indicator_lr_10.update_raw(1.00030);
        indicator_lr_10.update_raw(1.00010);
        indicator_lr_10.update_raw(1.00000);

        assert!((indicator_lr_10.value - 1.000_232_727_272_727_6).abs() < 1e-12);
    }

    #[rstest]
    fn r2_nan_for_constant_series() {
        let mut lr = LinearRegression::new(5);
        for _ in 0..5 {
            lr.update_raw(42.0);
        }
        assert!(lr.initialized);
        assert!(
            lr.r2.is_nan(),
            "R² should be NaN for a constant-value input series"
        );
    }

    #[rstest]
    fn cfo_nan_when_last_price_zero() {
        let mut lr = LinearRegression::new(3);
        lr.update_raw(1.0);
        lr.update_raw(2.0);
        lr.update_raw(0.0);
        assert!(lr.initialized);
        assert!(
            lr.cfo.is_nan(),
            "CFO should be NaN when the most-recent price equals zero"
        );
    }

    #[rstest]
    fn positive_slope_and_degree_for_uptrend() {
        let mut lr = LinearRegression::new(4);
        for v in 1..=4 {
            lr.update_raw(f64::from(v));
        }
        assert!(lr.slope > 0.0, "slope expected positive for up-trend");
        assert!(lr.degree > 0.0, "degree expected positive for up-trend");
    }

    #[rstest]
    fn negative_slope_and_degree_for_downtrend() {
        let mut lr = LinearRegression::new(4);
        for v in (1..=4).rev() {
            lr.update_raw(f64::from(v));
        }
        assert!(lr.slope < 0.0, "slope expected negative for down-trend");
        assert!(lr.degree < 0.0, "degree expected negative for down-trend");
    }

    #[rstest]
    fn not_initialized_until_enough_samples() {
        let mut lr = LinearRegression::new(6);
        for v in 0..5 {
            lr.update_raw(f64::from(v));
        }
        assert!(
            !lr.initialized,
            "indicator should remain uninitialised with fewer than `period` inputs"
        );
    }

    #[rstest]
    #[case(128)]
    #[case(1_024)]
    #[case(16_384)]
    fn large_period_initialisation_and_window_size(#[case] period: usize) {
        let mut lr = LinearRegression::new(period);
        for v in 0..period {
            lr.update_raw(v as f64);
        }
        assert!(
            lr.initialized,
            "indicator should initialise after exactly `period` samples"
        );
        assert_eq!(
            lr.inputs.len(),
            period,
            "internal window length must equal the configured period"
        );
    }

    #[rstest]
    fn cached_constants_correct() {
        let period = 10;
        let lr = LinearRegression::new(period);

        let n = period as f64;
        let expected_x_sum = 0.5 * n * (n + 1.0);
        let expected_x_mul_sum = expected_x_sum * 2.0f64.mul_add(n, 1.0) / 3.0;
        let expected_divisor = n.mul_add(expected_x_mul_sum, -(expected_x_sum * expected_x_sum));

        assert!((lr.x_sum - expected_x_sum).abs() < 1e-12, "x_sum mismatch");
        assert!(
            (lr.x_mul_sum - expected_x_mul_sum).abs() < 1e-12,
            "x_mul_sum mismatch"
        );
        assert!(
            (lr.divisor - expected_divisor).abs() < 1e-12,
            "divisor mismatch"
        );
    }

    #[rstest]
    fn cached_constants_immutable_through_updates() {
        let mut lr = LinearRegression::new(5);

        let (x_sum, x_mul_sum, divisor) = (lr.x_sum, lr.x_mul_sum, lr.divisor);

        for v in 0..20 {
            lr.update_raw(f64::from(v));
        }

        assert_eq!(lr.x_sum, x_sum, "x_sum must remain unchanged after updates");
        assert_eq!(
            lr.x_mul_sum, x_mul_sum,
            "x_mul_sum must remain unchanged after updates"
        );
        assert_eq!(
            lr.divisor, divisor,
            "divisor must remain unchanged after updates"
        );
    }

    #[rstest]
    fn cached_constants_immutable_after_reset() {
        let mut lr = LinearRegression::new(8);

        let (x_sum, x_mul_sum, divisor) = (lr.x_sum, lr.x_mul_sum, lr.divisor);

        for v in 0..8 {
            lr.update_raw(f64::from(v));
        }
        lr.reset();

        assert_eq!(lr.x_sum, x_sum, "x_sum must survive reset()");
        assert_eq!(lr.x_mul_sum, x_mul_sum, "x_mul_sum must survive reset()");
        assert_eq!(lr.divisor, divisor, "divisor must survive reset()");
    }

    const EPS: f64 = 1e-12;

    #[rstest]
    #[should_panic]
    fn new_zero_period_panics() {
        let _ = LinearRegression::new(0);
    }

    #[rstest]
    #[should_panic]
    fn new_period_exceeds_max_panics() {
        let _ = LinearRegression::new(MAX_PERIOD + 1);
    }

    #[rstest(
        period, value,
        case(8, 5.0),
        case(16, -3.1415)
    )]
    fn constant_non_zero_series(period: usize, value: f64) {
        let mut lr = LinearRegression::new(period);

        for _ in 0..period {
            lr.update_raw(value);
        }

        assert!(lr.initialized());
        assert!(lr.slope.abs() < EPS);
        assert!((lr.intercept - value).abs() < EPS);
        assert_eq!(lr.degree, 0.0);
        assert!(lr.r2.is_nan());
        assert!((lr.cfo).abs() < EPS);
        assert!((lr.value - value).abs() < EPS);
    }

    #[rstest(period, case(4), case(32))]
    fn constant_zero_series_cfo_nan(period: usize) {
        let mut lr = LinearRegression::new(period);

        for _ in 0..period {
            lr.update_raw(0.0);
        }

        assert!(lr.initialized());
        assert!(lr.cfo.is_nan());
    }

    #[rstest(period, case(6), case(13))]
    fn reset_clears_state_but_keeps_constants(period: usize) {
        let mut lr = LinearRegression::new(period);

        for i in 1..=period {
            lr.update_raw(i as f64);
        }

        let x_sum_before = lr.x_sum;
        let x_mul_sum_before = lr.x_mul_sum;
        let divisor_before = lr.divisor;

        lr.reset();

        assert!(!lr.initialized());
        assert!(!lr.has_inputs());

        assert!(lr.slope.abs() < EPS);
        assert!(lr.intercept.abs() < EPS);
        assert!(lr.degree.abs() < EPS);
        assert!(lr.cfo.abs() < EPS);
        assert!(lr.r2.abs() < EPS);
        assert!(lr.value.abs() < EPS);

        assert_eq!(lr.x_sum, x_sum_before);
        assert_eq!(lr.x_mul_sum, x_mul_sum_before);
        assert_eq!(lr.divisor, divisor_before);
    }

    #[rstest(period, case(5), case(31))]
    fn perfect_linear_series(period: usize) {
        const A: f64 = 2.0;
        const B: f64 = -3.0;
        let mut lr = LinearRegression::new(period);

        for x in 1..=period {
            lr.update_raw(A.mul_add(x as f64, B));
        }

        assert!(lr.initialized());
        assert!((lr.slope - A).abs() < EPS);
        assert!((lr.intercept - B).abs() < EPS);
        assert!((lr.r2 - 1.0).abs() < EPS);
        assert!((lr.degree.to_radians().tan() - A).abs() < EPS);
    }

    #[rstest]
    fn sliding_window_keeps_last_period() {
        const P: usize = 4;
        let mut lr = LinearRegression::new(P);
        for i in 1..=P {
            lr.update_raw(i as f64);
        }
        let slope_first_window = lr.slope;

        lr.update_raw(-100.0);
        assert!(lr.slope < slope_first_window);
        assert_eq!(lr.inputs.len(), P);
        assert_eq!(lr.inputs.front(), Some(&2.0));
    }

    #[rstest]
    fn r2_between_zero_and_one() {
        const P: usize = 32;
        let mut lr = LinearRegression::new(P);
        for x in 1..=P {
            let noise = if x % 2 == 0 { 0.5 } else { -0.5 };
            lr.update_raw(3.0f64.mul_add(x as f64, noise));
        }
        assert!(lr.r2 > 0.0 && lr.r2 < 1.0);
    }

    #[rstest]
    fn reset_before_initialized() {
        let mut lr = LinearRegression::new(10);
        lr.update_raw(1.0);
        lr.reset();

        assert!(!lr.initialized());
        assert!(!lr.has_inputs());
        assert_eq!(lr.inputs.len(), 0);
    }
}

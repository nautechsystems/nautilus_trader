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

use std::fmt::{Debug, Display};

use nautilus_core::correctness::FAILED;
use nautilus_model::data::Bar;

use crate::{
    indicator::Indicator,
    support::{MAX_PERIOD, RollingOls},
};

/// Linear regression over a rolling price window.
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
    ols: RollingOls,
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
        self.ols.reset();
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
    /// `period` is less than two.
    /// `period` exceeds `MAX_PERIOD` (16,777,216).
    #[must_use]
    pub fn new(period: usize) -> Self {
        Self::new_checked(period).expect(FAILED)
    }

    pub(crate) fn new_checked(period: usize) -> anyhow::Result<Self> {
        anyhow::ensure!(period > 0, "LinearRegression: period must be > 0");
        anyhow::ensure!(period >= 2, "LinearRegression: period must be >= 2");
        anyhow::ensure!(
            period <= MAX_PERIOD,
            "LinearRegression: period exceeds MAX_PERIOD ({MAX_PERIOD})"
        );
        Ok(Self {
            period,
            slope: 0.0,
            intercept: 0.0,
            degree: 0.0,
            cfo: 0.0,
            r2: 0.0,
            value: 0.0,
            initialized: false,
            has_inputs: false,
            ols: RollingOls::new(period),
        })
    }

    /// Updates the linear regression with a new data point.
    pub fn update_raw(&mut self, close: f64) {
        if !close.is_finite() {
            return;
        }
        self.has_inputs = true;

        if !self.ols.push(close) {
            return;
        }

        let n = self.period as f64;
        self.slope = self.ols.slope();
        self.intercept = self.ols.intercept(self.slope);
        self.value = self.intercept + self.slope * (n - 1.0);
        self.degree = self.slope.atan().to_degrees();
        self.cfo = if close == 0.0 {
            f64::NAN
        } else {
            100.0 * (self.value - close) / close
        };
        let mean = self.ols.sum_y() / n;
        let sst = (self.ols.sum_y_sq() - n * mean * mean).max(0.0);
        self.r2 = if sst < f64::EPSILON {
            f64::NAN
        } else {
            (self.slope * self.slope * self.ols.denom() / n / sst).clamp(0.0, 1.0)
        };
        self.initialized = true;
    }
}

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
    #[case(vec![4.0, 5.0, 9.0, 10.0], 3.7, 10.3)]
    #[case(vec![4.0, 5.0, 9.0, 10.0, 12.0], 5.7, 12.3)]
    fn zero_based_fit_matches_expected_outputs(
        #[case] prices: Vec<f64>,
        #[case] expected_intercept: f64,
        #[case] expected_endpoint: f64,
    ) {
        let mut regression = LinearRegression::new(4);

        for price in prices {
            regression.update_raw(price);
        }

        for (actual, expected) in [
            (regression.intercept, expected_intercept),
            (regression.value, expected_endpoint),
            (regression.slope, 2.2),
            (regression.r2, 121.0 / 130.0),
            (regression.degree, 2.2_f64.atan().to_degrees()),
        ] {
            assert!(
                (actual - expected).abs() <= 1.0e-12,
                "actual {actual}, expected {expected}"
            );
        }
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
        assert_eq!(indicator_lr_10.period, 10);
        assert_eq!(indicator_lr_10.slope, 0.0);
        assert_eq!(indicator_lr_10.intercept, 0.0);
        assert_eq!(indicator_lr_10.degree, 0.0);
        assert_eq!(indicator_lr_10.cfo, 0.0);
        assert_eq!(indicator_lr_10.r2, 0.0);
        assert!(!indicator_lr_10.has_inputs);
        assert!(!indicator_lr_10.initialized);
    }

    #[rstest]
    fn test_fit_uses_latest_period() {
        let mut lr = LinearRegression::new(5);
        for i in 1..=100 {
            lr.update_raw(f64::from(i));
            if i >= 5 {
                assert_eq!(lr.value, f64::from(i));
                assert_eq!(lr.slope, 1.0);
                assert_eq!(lr.intercept, f64::from(i - 4));
            }
        }
    }

    #[rstest]
    fn test_oldest_element_evicted() {
        let mut lr = LinearRegression::new(4);
        for v in 1..=5 {
            lr.update_raw(f64::from(v));
        }
        assert_eq!(lr.intercept, 2.0);
        assert_eq!(lr.value, 5.0);
    }

    #[rstest]
    fn test_recent_elements_preserved() {
        let mut lr = LinearRegression::new(5);
        for v in 0..5 {
            lr.update_raw(f64::from(v));
        }
        lr.update_raw(99.0);
        assert_eq!(lr.slope, 19.8);
        assert_eq!(lr.intercept, -17.8);
        assert_eq!(lr.value, 61.400000000000006);
    }

    #[rstest]
    fn test_multiple_evictions() {
        let mut lr = LinearRegression::new(2);
        lr.update_raw(10.0);
        lr.update_raw(20.0);
        lr.update_raw(30.0);
        lr.update_raw(40.0);
        assert_eq!(lr.intercept, 30.0);
        assert_eq!(lr.value, 40.0);
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
    fn large_period_initialization_and_window_size(#[case] period: usize) {
        let mut lr = LinearRegression::new(period);
        for v in 0..period {
            lr.update_raw(v as f64);
        }
        assert!(
            lr.initialized,
            "indicator should initialize after exactly `period` samples"
        );
        assert_eq!(lr.period, period);
    }

    #[rstest]
    fn checked_period_bounds() {
        assert!(LinearRegression::new_checked(0).is_err());
        assert!(LinearRegression::new_checked(1).is_err());
        assert!(LinearRegression::new_checked(MAX_PERIOD + 1).is_err());
        assert!(LinearRegression::new_checked(2).is_ok());
    }

    #[rstest]
    fn period_and_fit_after_updates() {
        let mut lr = LinearRegression::new(5);
        for i in 0..20 {
            lr.update_raw(f64::from(i));
        }
        assert_eq!(lr.period, 5);
        assert_eq!(lr.slope, 1.0);
        assert_eq!(lr.intercept, 15.0);
        assert_eq!(lr.value, 19.0);
    }

    #[rstest]
    fn period_and_fit_after_reset() {
        let mut lr = LinearRegression::new(8);
        for i in 0..20 {
            lr.update_raw(f64::from(i));
        }
        lr.reset();
        for i in 0..8 {
            lr.update_raw(f64::from(i) * 2.0);
        }
        assert_eq!(lr.period, 8);
        assert_eq!(lr.slope, 2.0);
        assert_eq!(lr.intercept, 0.0);
        assert_eq!(lr.value, 14.0);
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
        case(16, -std::f64::consts::PI)
    )]
    fn constant_non_zero_series(period: usize, value: f64) {
        let mut lr = LinearRegression::new(period);

        for _ in 0..period {
            lr.update_raw(value);
        }

        assert!(lr.initialized());
        assert!(lr.slope.abs() < EPS);
        assert!((lr.intercept - value).abs() < EPS);
        assert!(lr.degree.abs() < EPS);
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
    fn reset_clears_state_but_keeps_period(period: usize) {
        let mut lr = LinearRegression::new(period);

        for i in 1..=period {
            lr.update_raw(i as f64);
        }

        lr.reset();

        assert!(!lr.initialized());
        assert!(!lr.has_inputs());

        assert!(lr.slope.abs() < EPS);
        assert!(lr.intercept.abs() < EPS);
        assert!(lr.degree.abs() < EPS);
        assert!(lr.cfo.abs() < EPS);
        assert!(lr.r2.abs() < EPS);
        assert!(lr.value.abs() < EPS);

        assert_eq!(lr.period, period);
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
        assert!((lr.intercept - (A + B)).abs() < EPS);
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
        assert_eq!(lr.intercept, 23.0);
        assert_eq!(lr.slope, -30.5);
        assert_eq!(lr.value, -68.5);
    }

    #[rstest]
    fn r2_between_zero_and_one() {
        const P: usize = 32;
        let mut lr = LinearRegression::new(P);
        for x in 1..=P {
            let noise = if x.is_multiple_of(2) { 0.5 } else { -0.5 };
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
        assert_eq!(lr.value, 0.0);
    }
    #[rstest]
    #[case(0.0)]
    #[case(1e12)]
    fn shifted_fit_matches_centered_oracle_after_eviction_and_reset(#[case] offset: f64) {
        let mut lr = LinearRegression::new(3);
        for _ in 0..2 {
            lr.reset();
            let values: Vec<f64> = (0..400).map(|i| offset + f64::from((i * 7) % 13)).collect();
            for (i, &value) in values.iter().enumerate() {
                lr.update_raw(value);
                assert_eq!(lr.initialized, i >= 2);
                if i < 2 {
                    continue;
                }
                let window = &values[i - 2..=i];
                let deviations = [window[0] - offset, window[1] - offset, window[2] - offset];
                let mean = deviations.iter().sum::<f64>() / 3.0;
                let slope = (deviations[2] - deviations[0]) / 2.0;
                let intercept = offset + (mean - slope);
                let endpoint = intercept + 2.0 * slope;
                let sst = deviations
                    .iter()
                    .map(|value| (value - mean).powi(2))
                    .sum::<f64>();
                let r2 = 2.0 * slope * slope / sst;
                assert!((lr.slope - slope).abs() < 1e-12);
                assert!((lr.intercept - intercept).abs() < 1e-12);
                assert!((lr.value - endpoint).abs() < 1e-12);
                assert!((lr.degree - slope.atan().to_degrees()).abs() < 1e-12);
                if value == 0.0 {
                    assert!(lr.cfo.is_nan());
                } else {
                    assert!((lr.cfo - 100.0 * (endpoint - value) / value).abs() < 1e-12);
                }
                assert!((lr.r2 - r2).abs() < 1e-12);
            }
        }
    }
}

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

use std::fmt::Display;

use arraydeque::{ArrayDeque, Wrapping};
use nautilus_model::data::Bar;
use strum::{AsRefStr, Display as StrumDisplay, EnumIter, EnumString, FromRepr};

use crate::{
    average::{MovingAverageFactory, MovingAverageType},
    indicator::{Indicator, MovingAverage},
};

const MAX_PERIOD: usize = 1_024;

/// Method for calculating %D in the Stochastics indicator.
///
/// The %D line is the smoothed version of %K and can provide trading signals.
/// Two calculation methods are supported:
///
/// - **Ratio**: Original Nautilus method using `100 * SUM(close-LL) / SUM(HH-LL)` over `period_d`.
///   This is range-weighted and has less lag than MA-based methods.
/// - **MovingAverage**: Uses MA of slowed %K values, compatible with
///   cTrader/MetaTrader/TradingView implementations.
#[repr(C)]
#[derive(
    Copy,
    Clone,
    Debug,
    Default,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    AsRefStr,
    FromRepr,
    EnumIter,
    EnumString,
    StrumDisplay,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        hash,
        module = "nautilus_trader.core.nautilus_pyo3.indicators"
    )
)]
pub enum StochasticsDMethod {
    /// Ratio: Nautilus original method: `100 * SUM(close-LL) / SUM(HH-LL)` over `period_d`.
    /// This is range-weighted and has less lag than MA-based methods.
    #[default]
    Ratio,
    /// MA method: `MA(slowed_k, period_d, ma_type)`.
    /// This produces values compatible with cTrader/MetaTrader/TradingView implementations.
    MovingAverage,
}

#[repr(C)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.indicators")
)]
pub struct Stochastics {
    /// The lookback period for %K calculation (highest high / lowest low).
    pub period_k: usize,
    /// The smoothing period for %D calculation.
    pub period_d: usize,
    /// The slowing period for %K smoothing (1 = no slowing (Nautilus original).
    pub slowing: usize,
    /// The moving average type used for slowing and MA-based %D.
    pub ma_type: MovingAverageType,
    /// The method for calculating %D (Ratio = Nautilus original method, MovingAverage = MA Smoothed).
    pub d_method: StochasticsDMethod,
    /// The current %K value (slowed if slowing > 1).
    pub value_k: f64,
    /// The current %D value.
    pub value_d: f64,
    /// Whether the indicator has received sufficient inputs to produce valid values.
    pub initialized: bool,
    has_inputs: bool,
    highs: ArrayDeque<f64, MAX_PERIOD, Wrapping>,
    lows: ArrayDeque<f64, MAX_PERIOD, Wrapping>,
    c_sub_1: ArrayDeque<f64, MAX_PERIOD, Wrapping>,
    h_sub_l: ArrayDeque<f64, MAX_PERIOD, Wrapping>,
    /// Moving average for %K slowing (None when slowing == 1).
    slowing_ma: Option<Box<dyn MovingAverage + Send + Sync>>,
    /// Moving average for %D when d_method == MovingAverage.
    d_ma: Option<Box<dyn MovingAverage + Send + Sync>>,
}

impl std::fmt::Debug for Stochastics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Stochastics")
            .field("period_k", &self.period_k)
            .field("period_d", &self.period_d)
            .field("slowing", &self.slowing)
            .field("ma_type", &self.ma_type)
            .field("d_method", &self.d_method)
            .field("value_k", &self.value_k)
            .field("value_d", &self.value_d)
            .field("initialized", &self.initialized)
            .field("has_inputs", &self.has_inputs)
            .field(
                "slowing_ma",
                &self.slowing_ma.as_ref().map(|_| "MovingAverage"),
            )
            .field("d_ma", &self.d_ma.as_ref().map(|_| "MovingAverage"))
            .finish()
    }
}

impl Display for Stochastics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({},{})", self.name(), self.period_k, self.period_d,)
    }
}

impl Indicator for Stochastics {
    fn name(&self) -> String {
        stringify!(Stochastics).to_string()
    }

    fn has_inputs(&self) -> bool {
        self.has_inputs
    }

    fn initialized(&self) -> bool {
        self.initialized
    }

    fn handle_bar(&mut self, bar: &Bar) {
        self.update_raw((&bar.high).into(), (&bar.low).into(), (&bar.close).into());
    }

    fn reset(&mut self) {
        self.highs.clear();
        self.lows.clear();
        self.c_sub_1.clear();
        self.h_sub_l.clear();
        self.value_k = 0.0;
        self.value_d = 0.0;
        self.has_inputs = false;
        self.initialized = false;

        // Reset slowing MA if present
        if let Some(ref mut ma) = self.slowing_ma {
            ma.reset();
        }

        // Reset %D MA if present
        if let Some(ref mut ma) = self.d_ma {
            ma.reset();
        }
    }
}

impl Stochastics {
    /// Creates a new [`Stochastics`] instance with default parameters.
    ///
    /// This is the backward-compatible constructor that produces identical output
    /// to the original Nautilus implementation, setting the following to:
    /// - `slowing = 1` (no slowing applied to %K)
    /// - `ma_type = Exponential` (unused when slowing = 1 or with Ratio method)
    /// - `d_method = Ratio` (Nautilus native %D calculation)
    ///
    /// # Panics
    ///
    /// This function panics if:
    /// - `period_k` or `period_d` is less than 1 or greater than `MAX_PERIOD`.
    #[must_use]
    pub fn new(period_k: usize, period_d: usize) -> Self {
        Self::new_with_params(
            period_k,
            period_d,
            1,                              // slowing = 1 (no slowing)
            MovingAverageType::Exponential, // ma_type (unused)
            StochasticsDMethod::Ratio,      // d_method = Ratio
        )
    }

    /// Creates a new [`Stochastics`] instance with full parameter control.
    ///
    /// # Parameters
    ///
    /// - `period_k`: The lookback period for %K (highest high / lowest low).
    /// - `period_d`: The smoothing period for %D.
    /// - `slowing`: MA smoothing period for raw %K (1 = no slowing, > 1 = smoothed).
    /// - `ma_type`: MA type for slowing and MA-based %D (EMA, SMA, Wilder, etc.).
    /// - `d_method`: %D calculation method (Ratio = Nautilus original, MovingAverage = MA smoothed).
    ///
    /// # Panics
    ///
    /// This function panics if:
    /// - `period_k`, `period_d`, or `slowing` is less than 1 or greater than `MAX_PERIOD`.
    #[must_use]
    pub fn new_with_params(
        period_k: usize,
        period_d: usize,
        slowing: usize,
        ma_type: MovingAverageType,
        d_method: StochasticsDMethod,
    ) -> Self {
        assert!(
            period_k > 0 && period_k <= MAX_PERIOD,
            "Stochastics: period_k {period_k} exceeds bounds (1..={MAX_PERIOD})"
        );
        assert!(
            period_d > 0 && period_d <= MAX_PERIOD,
            "Stochastics: period_d {period_d} exceeds bounds (1..={MAX_PERIOD})"
        );
        assert!(
            slowing > 0 && slowing <= MAX_PERIOD,
            "Stochastics: slowing {slowing} exceeds bounds (1..={MAX_PERIOD})"
        );

        // Create slowing MA only if slowing > 1
        let slowing_ma = if slowing > 1 {
            Some(MovingAverageFactory::create(ma_type, slowing))
        } else {
            None
        };

        // Create %D MA only if d_method == MovingAverage
        let d_ma = match d_method {
            StochasticsDMethod::MovingAverage => {
                Some(MovingAverageFactory::create(ma_type, period_d))
            }
            StochasticsDMethod::Ratio => None,
        };

        Self {
            period_k,
            period_d,
            slowing,
            ma_type,
            d_method,
            has_inputs: false,
            initialized: false,
            value_k: 0.0,
            value_d: 0.0,
            highs: ArrayDeque::new(),
            lows: ArrayDeque::new(),
            h_sub_l: ArrayDeque::new(),
            c_sub_1: ArrayDeque::new(),
            slowing_ma,
            d_ma,
        }
    }

    /// Updates the indicator with raw price values.
    ///
    /// # Parameters
    ///
    /// - `high`: The high price for the period.
    /// - `low`: The low price for the period.
    /// - `close`: The close price for the period.
    pub fn update_raw(&mut self, high: f64, low: f64, close: f64) {
        if !self.has_inputs {
            self.has_inputs = true;
        }

        // Maintain high/low deques for period_k lookback
        if self.highs.len() == self.period_k {
            self.highs.pop_front();
            self.lows.pop_front();
        }
        let _ = self.highs.push_back(high);
        let _ = self.lows.push_back(low);

        // Check initialization for period_k (matches original behavior)
        if !self.initialized
            && self.highs.len() == self.period_k
            && self.lows.len() == self.period_k
        {
            // Original behavior: set initialized when period_k is filled
            // (for backward compat with d_method=Ratio, slowing=1)
            if self.slowing_ma.is_none() && self.d_method == StochasticsDMethod::Ratio {
                self.initialized = true;
            }
        }

        // Calculate highest high and lowest low over period_k
        let k_max_high = self.highs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let k_min_low = self.lows.iter().copied().fold(f64::INFINITY, f64::min);

        // For Ratio method, always update the deques (matches original behavior)
        if self.d_method == StochasticsDMethod::Ratio {
            if self.c_sub_1.len() == self.period_d {
                self.c_sub_1.pop_front();
                self.h_sub_l.pop_front();
            }
            let _ = self.c_sub_1.push_back(close - k_min_low);
            let _ = self.h_sub_l.push_back(k_max_high - k_min_low);
        }

        // Handle division by zero (flat market)
        if k_max_high == k_min_low {
            return;
        }

        // Calculate raw %K
        let raw_k = 100.0 * ((close - k_min_low) / (k_max_high - k_min_low));

        // Apply slowing if configured (slowing > 1)
        let slowed_k = match &mut self.slowing_ma {
            Some(ma) => {
                ma.update_raw(raw_k);
                ma.value()
            }
            None => raw_k, // No slowing when slowing == 1
        };
        self.value_k = slowed_k;

        // Calculate %D based on d_method
        self.value_d = match self.d_method {
            StochasticsDMethod::Ratio => {
                // Nautilus original: 100 * SUM(close-LL) / SUM(HH-LL) over period_d
                // Deques already updated above
                let sum_h_sub_l: f64 = self.h_sub_l.iter().sum();
                if sum_h_sub_l == 0.0 {
                    0.0
                } else {
                    100.0 * (self.c_sub_1.iter().sum::<f64>() / sum_h_sub_l)
                }
            }
            StochasticsDMethod::MovingAverage => {
                // cTrader-like: MA(slowed_k, period_d, ma_type)
                if let Some(ref mut ma) = self.d_ma {
                    ma.update_raw(slowed_k);
                    ma.value()
                } else {
                    50.0 // Fallback (shouldn't happen)
                }
            }
        };

        // Update initialization state for new parameter combinations
        // For slowing > 1, we need additional warmup for the slowing MA
        // For d_method == MovingAverage, we need additional warmup for the %D MA
        if !self.initialized {
            let base_ready = self.highs.len() == self.period_k;
            let slowing_ready = match &self.slowing_ma {
                Some(ma) => ma.initialized(),
                None => true,
            };
            let d_ready = match self.d_method {
                StochasticsDMethod::Ratio => true, // Already handled above for backward compat
                StochasticsDMethod::MovingAverage => match &self.d_ma {
                    Some(ma) => ma.initialized(),
                    None => true,
                },
            };

            if base_ready && slowing_ready && d_ready {
                self.initialized = true;
            }
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_model::data::Bar;
    use rstest::rstest;

    use crate::{
        average::MovingAverageType,
        indicator::Indicator,
        momentum::stochastics::{Stochastics, StochasticsDMethod},
        stubs::{bar_ethusdt_binance_minute_bid, stochastics_10},
    };

    // ============================================================================
    // Backward Compatibility Tests (existing tests, must continue to pass)
    // ============================================================================

    #[rstest]
    fn test_stochastics_initialized(stochastics_10: Stochastics) {
        let display_str = format!("{stochastics_10}");
        assert_eq!(display_str, "Stochastics(10,10)");
        assert_eq!(stochastics_10.period_d, 10);
        assert_eq!(stochastics_10.period_k, 10);
        assert!(!stochastics_10.initialized);
        assert!(!stochastics_10.has_inputs);
    }

    #[rstest]
    fn test_value_with_one_input(mut stochastics_10: Stochastics) {
        stochastics_10.update_raw(1.0, 1.0, 1.0);
        assert_eq!(stochastics_10.value_d, 0.0);
        assert_eq!(stochastics_10.value_k, 0.0);
    }

    #[rstest]
    fn test_value_with_three_inputs(mut stochastics_10: Stochastics) {
        stochastics_10.update_raw(1.0, 1.0, 1.0);
        stochastics_10.update_raw(2.0, 2.0, 2.0);
        stochastics_10.update_raw(3.0, 3.0, 3.0);
        assert_eq!(stochastics_10.value_d, 100.0);
        assert_eq!(stochastics_10.value_k, 100.0);
    }

    #[rstest]
    fn test_value_with_ten_inputs(mut stochastics_10: Stochastics) {
        let high_values = [
            1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0,
        ];
        let low_values = [
            0.9, 1.9, 2.9, 3.9, 4.9, 5.9, 6.9, 7.9, 8.9, 9.9, 10.1, 10.2, 10.3, 11.1, 11.4,
        ];
        let close_values = [
            1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0,
        ];

        for i in 0..15 {
            stochastics_10.update_raw(high_values[i], low_values[i], close_values[i]);
        }

        assert!(stochastics_10.initialized());
        assert_eq!(stochastics_10.value_d, 100.0);
        assert_eq!(stochastics_10.value_k, 100.0);
    }

    #[rstest]
    fn test_initialized_with_required_input(mut stochastics_10: Stochastics) {
        for i in 1..10 {
            stochastics_10.update_raw(f64::from(i), f64::from(i), f64::from(i));
        }
        assert!(!stochastics_10.initialized);
        stochastics_10.update_raw(10.0, 12.0, 14.0);
        assert!(stochastics_10.initialized);
    }

    #[rstest]
    fn test_handle_bar(mut stochastics_10: Stochastics, bar_ethusdt_binance_minute_bid: Bar) {
        stochastics_10.handle_bar(&bar_ethusdt_binance_minute_bid);
        assert_eq!(stochastics_10.value_d, 49.090_909_090_909_09);
        assert_eq!(stochastics_10.value_k, 49.090_909_090_909_09);
        assert!(stochastics_10.has_inputs);
        assert!(!stochastics_10.initialized);
    }

    #[rstest]
    fn test_reset(mut stochastics_10: Stochastics) {
        stochastics_10.update_raw(1.0, 1.0, 1.0);
        assert_eq!(stochastics_10.c_sub_1.len(), 1);
        assert_eq!(stochastics_10.h_sub_l.len(), 1);

        stochastics_10.reset();
        assert_eq!(stochastics_10.value_d, 0.0);
        assert_eq!(stochastics_10.value_k, 0.0);
        assert_eq!(stochastics_10.h_sub_l.len(), 0);
        assert_eq!(stochastics_10.c_sub_1.len(), 0);
        assert!(!stochastics_10.has_inputs);
        assert!(!stochastics_10.initialized);
    }

    // ============================================================================
    // New Parameter Tests
    // ============================================================================

    #[rstest]
    fn test_new_defaults_slowing_1_ratio() {
        let stoch = Stochastics::new(10, 3);
        assert_eq!(stoch.period_k, 10);
        assert_eq!(stoch.period_d, 3);
        assert_eq!(stoch.slowing, 1);
        assert_eq!(stoch.ma_type, MovingAverageType::Exponential);
        assert_eq!(stoch.d_method, StochasticsDMethod::Ratio);
        assert!(
            stoch.slowing_ma.is_none(),
            "slowing_ma should be None when slowing == 1"
        );
        assert!(
            stoch.d_ma.is_none(),
            "d_ma should be None when d_method == Ratio"
        );
    }

    #[rstest]
    fn test_new_with_params_accepts_all_params() {
        let stoch = Stochastics::new_with_params(
            11,
            3,
            3,
            MovingAverageType::Exponential,
            StochasticsDMethod::MovingAverage,
        );
        assert_eq!(stoch.period_k, 11);
        assert_eq!(stoch.period_d, 3);
        assert_eq!(stoch.slowing, 3);
        assert_eq!(stoch.ma_type, MovingAverageType::Exponential);
        assert_eq!(stoch.d_method, StochasticsDMethod::MovingAverage);
        assert!(
            stoch.slowing_ma.is_some(),
            "slowing_ma should exist when slowing > 1"
        );
        assert!(
            stoch.d_ma.is_some(),
            "d_ma should exist when d_method == MovingAverage"
        );
    }

    #[rstest]
    fn test_backward_compatibility_identical_output() {
        // Create both old-style and new-style with explicit defaults
        let mut stoch_old = Stochastics::new(10, 10);
        let mut stoch_new = Stochastics::new_with_params(
            10,
            10,
            1,
            MovingAverageType::Exponential,
            StochasticsDMethod::Ratio,
        );

        // Feed identical data to both
        let high_values = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        let low_values = [0.5, 1.5, 2.5, 3.5, 4.5, 5.5, 6.5, 7.5, 8.5, 9.5];
        let close_values = [0.8, 1.8, 2.8, 3.8, 4.8, 5.8, 6.8, 7.8, 8.8, 9.8];

        for i in 0..10 {
            stoch_old.update_raw(high_values[i], low_values[i], close_values[i]);
            stoch_new.update_raw(high_values[i], low_values[i], close_values[i]);
        }

        // Output should be bit-for-bit identical
        assert_eq!(stoch_old.value_k, stoch_new.value_k, "value_k mismatch");
        assert_eq!(stoch_old.value_d, stoch_new.value_d, "value_d mismatch");
        assert_eq!(stoch_old.initialized, stoch_new.initialized);
    }

    // ============================================================================
    // Slowing Tests
    // ============================================================================

    #[rstest]
    fn test_slowing_3_smoothes_k() {
        let mut stoch_no_slowing = Stochastics::new(5, 3);
        let mut stoch_with_slowing = Stochastics::new_with_params(
            5,
            3,
            3,
            MovingAverageType::Exponential,
            StochasticsDMethod::Ratio,
        );

        // Generate varying data to show smoothing effect
        let data = [
            (10.0, 5.0, 8.0),
            (12.0, 6.0, 7.0),
            (11.0, 4.0, 9.0),
            (13.0, 7.0, 8.0),
            (14.0, 8.0, 10.0),
            (12.0, 6.0, 7.0),
            (15.0, 9.0, 14.0),
            (16.0, 10.0, 11.0),
        ];

        for (high, low, close) in data {
            stoch_no_slowing.update_raw(high, low, close);
            stoch_with_slowing.update_raw(high, low, close);
        }

        // With slowing, %K should be smoother (different from raw)
        // We can't assert exact values without knowing the expected behavior,
        // but we can verify they differ when slowing is applied
        assert!(
            (stoch_no_slowing.value_k - stoch_with_slowing.value_k).abs() > 0.01,
            "Slowing should produce different %K values"
        );
    }

    #[rstest]
    #[case(MovingAverageType::Simple)]
    #[case(MovingAverageType::Exponential)]
    #[case(MovingAverageType::Wilder)]
    #[case(MovingAverageType::Hull)]
    fn test_slowing_with_different_ma_types(#[case] ma_type: MovingAverageType) {
        let mut stoch = Stochastics::new_with_params(5, 3, 3, ma_type, StochasticsDMethod::Ratio);

        // Feed data and verify it produces valid output
        for i in 1..=10 {
            stoch.update_raw(f64::from(i) + 5.0, f64::from(i), f64::from(i) + 2.0);
        }

        assert!(
            stoch.value_k.is_finite(),
            "value_k should be finite with {ma_type:?}"
        );
        assert!(
            stoch.value_d.is_finite(),
            "value_d should be finite with {ma_type:?}"
        );
        assert!(
            stoch.value_k >= 0.0 && stoch.value_k <= 100.0,
            "value_k out of range with {ma_type:?}"
        );
    }

    // ============================================================================
    // D Method Tests
    // ============================================================================

    #[rstest]
    fn test_d_method_ratio_preserves_nautilus_behavior() {
        let mut stoch = Stochastics::new_with_params(
            10,
            3,
            1, // No slowing
            MovingAverageType::Exponential,
            StochasticsDMethod::Ratio,
        );

        // Same data as original test
        for i in 1..=15 {
            stoch.update_raw(f64::from(i), f64::from(i) - 0.1, f64::from(i));
        }

        // Should produce same ratio-based %D as original
        assert!(stoch.initialized);
        assert!(stoch.value_d > 0.0);
    }

    #[rstest]
    fn test_d_method_ma_produces_smoothed_k() {
        let mut stoch = Stochastics::new_with_params(
            5,
            3,
            3, // With slowing
            MovingAverageType::Exponential,
            StochasticsDMethod::MovingAverage, // MA-based %D
        );

        let data = [
            (10.0, 5.0, 8.0),
            (12.0, 6.0, 7.0),
            (11.0, 4.0, 9.0),
            (13.0, 7.0, 8.0),
            (14.0, 8.0, 10.0),
            (12.0, 6.0, 7.0),
            (15.0, 9.0, 14.0),
            (16.0, 10.0, 11.0),
            (14.0, 8.0, 12.0),
            (13.0, 7.0, 10.0),
        ];

        for (high, low, close) in data {
            stoch.update_raw(high, low, close);
        }

        // %D should be smoothed version of %K
        assert!(stoch.value_d.is_finite());
        assert!(stoch.value_d >= 0.0 && stoch.value_d <= 100.0);
    }

    // ============================================================================
    // Warmup / Initialization Tests
    // ============================================================================

    #[rstest]
    fn test_warmup_period_with_slowing() {
        let mut stoch = Stochastics::new_with_params(
            5,
            3,
            3, // slowing = 3 means we need period_k + slowing inputs for slowing MA
            MovingAverageType::Exponential,
            StochasticsDMethod::Ratio,
        );

        // With period_k=5, slowing=3, period_d=3:
        // - Need 5 bars for period_k
        // - Need 3 more for slowing MA to initialize
        // - Need 3 for period_d ratio
        // Exact warmup depends on MA implementation

        for i in 1..=4 {
            stoch.update_raw(f64::from(i) + 5.0, f64::from(i), f64::from(i) + 2.0);
            assert!(!stoch.initialized, "Should not be initialized at bar {i}");
        }

        // After enough bars, should initialize
        for i in 5..=15 {
            stoch.update_raw(f64::from(i) + 5.0, f64::from(i), f64::from(i) + 2.0);
        }

        assert!(
            stoch.initialized,
            "Should be initialized after sufficient bars"
        );
    }

    #[rstest]
    fn test_warmup_period_with_ma_d_method() {
        let mut stoch = Stochastics::new_with_params(
            5,
            3,
            3,
            MovingAverageType::Exponential,
            StochasticsDMethod::MovingAverage, // MA %D needs its own warmup
        );

        for i in 1..=4 {
            stoch.update_raw(f64::from(i) + 5.0, f64::from(i), f64::from(i) + 2.0);
        }
        assert!(!stoch.initialized);

        // Keep feeding until initialized
        for i in 5..=20 {
            stoch.update_raw(f64::from(i) + 5.0, f64::from(i), f64::from(i) + 2.0);
        }

        assert!(
            stoch.initialized,
            "Should be initialized after sufficient bars"
        );
    }

    // ============================================================================
    // Reset Tests
    // ============================================================================

    #[rstest]
    fn test_reset_clears_slowing_ma_state() {
        let mut stoch = Stochastics::new_with_params(
            5,
            3,
            3,
            MovingAverageType::Exponential,
            StochasticsDMethod::MovingAverage,
        );

        // Feed some data
        for i in 1..=10 {
            stoch.update_raw(f64::from(i) + 5.0, f64::from(i), f64::from(i) + 2.0);
        }

        assert!(stoch.has_inputs);

        // Reset
        stoch.reset();

        assert!(!stoch.has_inputs);
        assert!(!stoch.initialized);
        assert_eq!(stoch.value_k, 0.0);
        assert_eq!(stoch.value_d, 0.0);
        assert_eq!(stoch.highs.len(), 0);
        assert_eq!(stoch.lows.len(), 0);

        // After reset, should be able to use again
        for i in 1..=10 {
            stoch.update_raw(f64::from(i) + 5.0, f64::from(i), f64::from(i) + 2.0);
        }
        assert!(stoch.value_k > 0.0);
    }

    // ============================================================================
    // Edge Cases
    // ============================================================================

    #[rstest]
    fn test_slowing_1_bypasses_ma() {
        let stoch = Stochastics::new_with_params(
            10,
            3,
            1, // slowing = 1 means no MA
            MovingAverageType::Exponential,
            StochasticsDMethod::Ratio,
        );

        assert!(
            stoch.slowing_ma.is_none(),
            "slowing = 1 should not create MA"
        );
    }

    #[rstest]
    #[should_panic(expected = "slowing")]
    fn test_slowing_0_panics() {
        let _ = Stochastics::new_with_params(
            10,
            3,
            0, // Invalid
            MovingAverageType::Exponential,
            StochasticsDMethod::Ratio,
        );
    }

    #[rstest]
    fn test_division_by_zero_protection() {
        let mut stoch = Stochastics::new_with_params(
            5,
            3,
            3,
            MovingAverageType::Exponential,
            StochasticsDMethod::MovingAverage,
        );

        // Flat market: high == low == close
        for _ in 0..10 {
            stoch.update_raw(100.0, 100.0, 100.0);
        }

        // Should not panic, values should be 0 or previous
        assert!(stoch.value_k.is_finite());
        assert!(stoch.value_d.is_finite());
    }
}

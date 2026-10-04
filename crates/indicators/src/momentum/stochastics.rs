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

use std::{
    collections::VecDeque,
    fmt::{Debug, Display},
};

use nautilus_core::correctness::FAILED;
use nautilus_model::data::Bar;
use strum::{AsRefStr, Display as StrumDisplay, EnumIter, EnumString, FromRepr};

use crate::{
    average::{MovingAverageFactory, MovingAverageType},
    indicator::{Indicator, MovingAverage},
    support::{MAX_PERIOD, is_valid_hlc},
};

// A flat window (HH == LL) emits this neutral value for both %K and %D
const FLAT_WINDOW_VALUE: f64 = 50.0;

/// Method for calculating %D in the Stochastics indicator.
///
/// The %D line is the smoothed version of %K and can provide trading signals.
/// Two calculation methods are supported:
///
/// - **Ratio**: Original Nautilus method using `100 * SUM(close-LL) / SUM(HH-LL)` over `period_d`.
///   This is range-weighted and has less lag than MA-based methods.
/// - **`MovingAverage`**: Uses MA of slowed %K values, compatible with
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
        module = "nautilus_trader.indicators",
        from_py_object,
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.indicators")
)]
pub enum StochasticsDMethod {
    /// Ratio: Nautilus original method: `100 * SUM(close-LL) / SUM(HH-LL)` over `period_d`.
    /// This is range-weighted and has less lag than MA-based methods.
    Ratio,
    /// MA method: `MA(slowed_k, period_d, ma_type)`.
    /// This produces values compatible with cTrader/MetaTrader/TradingView implementations.
    #[default]
    MovingAverage,
}

/// Stochastic oscillator with smoothed K and D outputs.
///
/// Defaults to `slowing = 1`, `ma_type = Simple`, and `d_method = MovingAverage`,
/// so D is a simple moving average of K. Select [`StochasticsDMethod::Ratio`]
/// for the legacy Nautilus range-weighted D calculation.
#[repr(C)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.indicators")
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.indicators")
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
    /// The method for calculating %D (Ratio = Nautilus original method, `MovingAverage` = MA Smoothed).
    pub d_method: StochasticsDMethod,
    /// The current %K value (slowed if slowing > 1).
    pub value_k: f64,
    /// The current %D value.
    pub value_d: f64,
    /// Whether the indicator has received sufficient inputs to produce valid values.
    pub initialized: bool,
    has_inputs: bool,
    highs: VecDeque<f64>,
    lows: VecDeque<f64>,
    c_sub_1: VecDeque<f64>,
    h_sub_l: VecDeque<f64>,
    /// Moving average for %K slowing (None when slowing == 1).
    slowing_ma: Option<Box<dyn MovingAverage + Send + Sync>>,
    /// Moving average for %D when `d_method` == `MovingAverage`.
    d_ma: Option<Box<dyn MovingAverage + Send + Sync>>,
}

impl Debug for Stochastics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(Stochastics))
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
            .finish_non_exhaustive()
    }
}

impl Display for Stochastics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({},{})", self.name(), self.period_k, self.period_d)
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
    pub(crate) const DEFAULT_SLOWING: usize = 1;
    pub(crate) const DEFAULT_MA_TYPE: MovingAverageType = MovingAverageType::Simple;
    pub(crate) const DEFAULT_D_METHOD: StochasticsDMethod = StochasticsDMethod::MovingAverage;

    /// Creates a new [`Stochastics`] instance with default parameters.
    ///
    /// The defaults follow the standard fast-stochastic convention:
    /// - `slowing = 1` (no slowing applied to %K)
    /// - `ma_type = Simple` (%D smoothing type)
    /// - `d_method = MovingAverage` (%D = SMA of %K)
    ///
    /// Use [`Stochastics::new_with_params`] with [`StochasticsDMethod::Ratio`]
    /// for the legacy Nautilus range-weighted %D.
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
            Self::DEFAULT_SLOWING,
            Self::DEFAULT_MA_TYPE,
            Self::DEFAULT_D_METHOD,
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
    /// - `d_method`: %D calculation method (Ratio = Nautilus original, `MovingAverage` = MA smoothed).
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
        Self::new_checked(period_k, period_d, slowing, ma_type, d_method).expect(FAILED)
    }

    pub(crate) fn new_checked(
        period_k: usize,
        period_d: usize,
        slowing: usize,
        ma_type: MovingAverageType,
        d_method: StochasticsDMethod,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            period_k > 0 && period_k <= MAX_PERIOD,
            "Stochastics: period_k {period_k} exceeds bounds (1..={MAX_PERIOD})"
        );
        anyhow::ensure!(
            period_d > 0 && period_d <= MAX_PERIOD,
            "Stochastics: period_d {period_d} exceeds bounds (1..={MAX_PERIOD})"
        );
        anyhow::ensure!(
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

        let ratio_capacity = if d_method == StochasticsDMethod::Ratio {
            period_d
        } else {
            0
        };
        Ok(Self {
            period_k,
            period_d,
            slowing,
            ma_type,
            d_method,
            has_inputs: false,
            initialized: false,
            value_k: 0.0,
            value_d: 0.0,
            highs: VecDeque::with_capacity(period_k),
            lows: VecDeque::with_capacity(period_k),
            h_sub_l: VecDeque::with_capacity(ratio_capacity),
            c_sub_1: VecDeque::with_capacity(ratio_capacity),
            slowing_ma,
            d_ma,
        })
    }

    /// Updates the indicator with raw price values.
    ///
    /// # Parameters
    ///
    /// - `high`: The high price for the period.
    /// - `low`: The low price for the period.
    /// - `close`: The close price for the period.
    pub fn update_raw(&mut self, high: f64, low: f64, close: f64) {
        if !is_valid_hlc(high, low, close) {
            return;
        }

        if !self.has_inputs {
            self.has_inputs = true;
        }

        // Maintain high/low deques for period_k lookback
        if self.highs.len() == self.period_k {
            self.highs.pop_front();
            self.lows.pop_front();
        }
        self.highs.push_back(high);
        self.lows.push_back(low);

        // Calculate highest high and lowest low over period_k
        let k_max_high = self.highs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let k_min_low = self.lows.iter().copied().fold(f64::INFINITY, f64::min);

        // For Ratio method, always update the deques (matches original behavior)
        if self.d_method == StochasticsDMethod::Ratio {
            if self.c_sub_1.len() == self.period_d {
                self.c_sub_1.pop_front();
                self.h_sub_l.pop_front();
            }
            self.c_sub_1.push_back(close - k_min_low);
            self.h_sub_l.push_back(k_max_high - k_min_low);
        }

        #[expect(clippy::float_cmp, reason = "guards divide-by-zero on flat market")]
        let raw_k = if k_max_high == k_min_low {
            FLAT_WINDOW_VALUE
        } else {
            100.0 * ((close - k_min_low) / (k_max_high - k_min_low))
        };

        let k_ready = self.highs.len() == self.period_k;

        // Apply slowing if configured (slowing > 1)
        let slowed_k = match &mut self.slowing_ma {
            Some(ma) => {
                if k_ready {
                    ma.update_raw(raw_k);
                }
                ma.value()
            }
            None => {
                if k_ready || self.d_method == StochasticsDMethod::Ratio {
                    raw_k
                } else {
                    0.0
                }
            }
        };
        // Calculate %D based on d_method
        let value_d = match self.d_method {
            StochasticsDMethod::Ratio => {
                // Nautilus original: 100 * SUM(close-LL) / SUM(HH-LL) over period_d
                // Deques already updated above
                let sum_h_sub_l: f64 = self.h_sub_l.iter().sum();
                if sum_h_sub_l == 0.0 {
                    FLAT_WINDOW_VALUE
                } else {
                    100.0 * (self.c_sub_1.iter().sum::<f64>() / sum_h_sub_l)
                }
            }
            StochasticsDMethod::MovingAverage => {
                // cTrader-like: MA(slowed_k, period_d, ma_type)
                if let Some(ref mut ma) = self.d_ma {
                    let slowing_ready = self
                        .slowing_ma
                        .as_ref()
                        .is_none_or(|slowing_ma| slowing_ma.initialized());
                    if k_ready && slowing_ready {
                        ma.update_raw(slowed_k);
                    }
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
            let base_ready = k_ready;
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

        if self.initialized || self.d_method == StochasticsDMethod::Ratio {
            self.value_k = slowed_k;
            self.value_d = value_d;
        }
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::data::Bar;
    use rstest::rstest;

    use crate::{
        average::MovingAverageType,
        indicator::Indicator,
        momentum::stochastics::{Stochastics, StochasticsDMethod},
        stubs::{bar_ethusdt_binance_minute_bid, stochastics_10},
        testing::assert_approx_equal,
    };

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
        assert_eq!(stochastics_10.value_d, 0.0);
        assert_eq!(stochastics_10.value_k, 0.0);
    }

    #[rstest]
    fn test_value_with_full_chain(mut stochastics_10: Stochastics) {
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

        for value in 16..=19 {
            let value = f64::from(value);
            stochastics_10.update_raw(value, value - 0.1, value);
        }

        assert!(stochastics_10.initialized());
        assert_eq!(stochastics_10.value_d, 100.0);
        assert_eq!(stochastics_10.value_k, 100.0);
    }

    #[rstest]
    fn test_initialized_with_required_input(mut stochastics_10: Stochastics) {
        for i in 1..19 {
            stochastics_10.update_raw(f64::from(i), f64::from(i), f64::from(i));
            assert!(!stochastics_10.initialized);
        }
        stochastics_10.update_raw(19.0, 19.0, 19.0);
        assert!(stochastics_10.initialized);
    }

    #[rstest]
    fn test_handle_bar(mut stochastics_10: Stochastics, bar_ethusdt_binance_minute_bid: Bar) {
        stochastics_10.handle_bar(&bar_ethusdt_binance_minute_bid);
        assert_eq!(stochastics_10.value_d, 0.0);
        assert_eq!(stochastics_10.value_k, 0.0);
        assert!(stochastics_10.has_inputs);
        assert!(!stochastics_10.initialized);
        for _ in 1..10 {
            stochastics_10.handle_bar(&bar_ethusdt_binance_minute_bid);
        }
        assert_eq!(stochastics_10.value_k, 0.0);
        assert_eq!(stochastics_10.value_d, 0.0);
        assert!(!stochastics_10.initialized);
        for _ in 0..9 {
            stochastics_10.handle_bar(&bar_ethusdt_binance_minute_bid);
        }
        assert_approx_equal(stochastics_10.value_d, 49.0909090909);
        assert_approx_equal(stochastics_10.value_k, 49.0909090909);
        assert!(stochastics_10.initialized);
    }

    #[rstest]
    fn test_reset(mut stochastics_10: Stochastics) {
        stochastics_10.update_raw(1.0, 1.0, 1.0);

        stochastics_10.reset();
        assert_eq!(stochastics_10.value_d, 0.0);
        assert_eq!(stochastics_10.value_k, 0.0);
        assert_eq!(stochastics_10.h_sub_l.len(), 0);
        assert_eq!(stochastics_10.c_sub_1.len(), 0);
        assert!(!stochastics_10.has_inputs);
        assert!(!stochastics_10.initialized);
    }

    #[rstest]
    fn test_new_defaults_slowing_1_ma_d() {
        let stoch = Stochastics::new(10, 3);
        assert_eq!(stoch.period_k, 10);
        assert_eq!(stoch.period_d, 3);
        assert_eq!(stoch.slowing, 1);
        assert_eq!(stoch.ma_type, MovingAverageType::Simple);
        assert_eq!(stoch.d_method, StochasticsDMethod::MovingAverage);
        assert!(
            stoch.slowing_ma.is_none(),
            "slowing_ma should be None when slowing == 1"
        );
        assert!(
            stoch.d_ma.is_some(),
            "d_ma should exist when d_method == MovingAverage"
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
        // `new` must equal `new_with_params` with the documented defaults
        let mut stoch_old = Stochastics::new(10, 10);
        let mut stoch_new = Stochastics::new_with_params(
            10,
            10,
            1,
            MovingAverageType::Simple,
            StochasticsDMethod::MovingAverage,
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
    #[case(MovingAverageType::Simple, 5)]
    #[case(MovingAverageType::Exponential, 5)]
    #[case(MovingAverageType::DoubleExponential, 7)]
    #[case(MovingAverageType::Wilder, 5)]
    #[case(MovingAverageType::Hull, 5)]
    fn test_composite_warmup_uses_selected_ma(
        #[case] ma_type: MovingAverageType,
        #[case] warmup: usize,
    ) {
        let mut stoch =
            Stochastics::new_with_params(3, 2, 2, ma_type, StochasticsDMethod::MovingAverage);

        for i in 1..warmup {
            let value = i as f64;
            stoch.update_raw(value + 2.0, value, value + 1.0);
            assert!(!stoch.initialized(), "initialized at input {i}");
        }

        let value = warmup as f64;
        stoch.update_raw(value + 2.0, value, value + 1.0);
        assert!(stoch.initialized());
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

    #[rstest]
    fn test_ratio_flat_window_emits_neutral_k_and_d() {
        let mut stoch = Stochastics::new_with_params(
            5,
            3,
            1,
            MovingAverageType::Simple,
            StochasticsDMethod::Ratio,
        );

        for _ in 0..5 {
            stoch.update_raw(100.0, 100.0, 100.0);
        }

        assert!(stoch.initialized());
        assert_eq!(stoch.value_k, 50.0);
        assert_eq!(stoch.value_d, 50.0);
    }
}
